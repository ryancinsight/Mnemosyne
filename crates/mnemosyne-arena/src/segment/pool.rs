//! Global segment pool management.

use crate::numa::current_numa_node;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::types::Segment;

#[repr(align(64))]
struct CacheAlignedAtomicUsize {
    value: core::sync::atomic::AtomicUsize,
}

impl CacheAlignedAtomicUsize {
    #[inline(always)]
    const fn new(val: usize) -> Self {
        Self {
            value: core::sync::atomic::AtomicUsize::new(val),
        }
    }
}

#[cfg(not(target_pointer_width = "64"))]
#[repr(align(64))]
struct CacheAlignedAtomicPtr<T> {
    value: core::sync::atomic::AtomicPtr<T>,
}

#[cfg(not(target_pointer_width = "64"))]
impl<T> CacheAlignedAtomicPtr<T> {
    #[inline(always)]
    const fn new(val: *mut T) -> Self {
        Self {
            value: core::sync::atomic::AtomicPtr::new(val),
        }
    }
}

/// A lock-free segment pool for a single NUMA node.
#[cfg(target_pointer_width = "64")]
pub struct NodeSegmentPool {
    head: CacheAlignedAtomicUsize,
    retained: CacheAlignedAtomicUsize,
    purged: core::sync::atomic::AtomicUsize,
    purge_calls: core::sync::atomic::AtomicUsize,
    reset_segments: core::sync::atomic::AtomicUsize,
    reset_calls: core::sync::atomic::AtomicUsize,
}

#[cfg(not(target_pointer_width = "64"))]
pub struct NodeSegmentPool {
    head: CacheAlignedAtomicPtr<Segment>,
    retained: CacheAlignedAtomicUsize,
    purged: core::sync::atomic::AtomicUsize,
    purge_calls: core::sync::atomic::AtomicUsize,
    reset_segments: core::sync::atomic::AtomicUsize,
    reset_calls: core::sync::atomic::AtomicUsize,
}

#[cfg(target_pointer_width = "64")]
impl NodeSegmentPool {
    /// Low bits reserved for the packed segment address.
    const PACKED_PTR_BITS: u32 = 48;
    /// Mask selecting the packed address bits.
    const PTR_MASK: usize = (1usize << Self::PACKED_PTR_BITS) - 1;
    /// Mask wrapping the push counter to the remaining high bits.
    const COUNT_WRAP_MASK: usize = (1usize << (usize::BITS - Self::PACKED_PTR_BITS)) - 1;
}

impl Default for NodeSegmentPool {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl NodeSegmentPool {
    /// Creates a new empty `NodeSegmentPool`.
    pub const fn new() -> Self {
        #[cfg(target_pointer_width = "64")]
        {
            Self {
                head: CacheAlignedAtomicUsize::new(0),
                retained: CacheAlignedAtomicUsize::new(0),
                purged: AtomicUsize::new(0),
                purge_calls: AtomicUsize::new(0),
                reset_segments: AtomicUsize::new(0),
                reset_calls: AtomicUsize::new(0),
            }
        }
        #[cfg(not(target_pointer_width = "64"))]
        {
            Self {
                head: CacheAlignedAtomicPtr::new(core::ptr::null_mut()),
                retained: CacheAlignedAtomicUsize::new(0),
                purged: AtomicUsize::new(0),
                purge_calls: AtomicUsize::new(0),
                reset_segments: AtomicUsize::new(0),
                reset_calls: AtomicUsize::new(0),
            }
        }
    }

    /// Pushes a segment back to the pool without applying a retention limit.
    ///
    /// # Safety
    ///
    /// The `segment` pointer must be a valid, initialized, and exclusive pointer to a
    /// `Segment` structure. The caller must transfer ownership of that segment back to
    /// the pool.
    #[inline]
    pub unsafe fn push_unbounded(&self, segment: *mut Segment) {
        self.retained.value.fetch_add(1, Ordering::Relaxed);
        self.push_raw(segment);
    }

    /// Pushes a segment back to the bounded reusable segment pool.
    ///
    /// Returns `true` if the segment was successfully cached, or `false` if the pool
    /// is already full.
    ///
    /// # Safety
    ///
    /// The `segment` pointer must be a valid, initialized, and exclusive pointer to a
    /// `Segment` structure. The caller must transfer ownership of that segment back to
    /// the pool.
    #[inline]
    pub unsafe fn try_push_retained(&self, segment: *mut Segment) -> bool {
        let mut retained = self.retained.value.load(Ordering::Relaxed);
        loop {
            if retained >= mnemosyne_core::options::MAX_RETAINED_SEGMENTS.load(Ordering::Relaxed) {
                return false;
            }
            match self.retained.value.compare_exchange_weak(
                retained,
                retained + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.push_raw(segment);
                    return true;
                }
                Err(actual) => retained = actual,
            }
        }
    }

    #[cfg(target_pointer_width = "64")]
    #[inline]
    fn push_raw(&self, segment: *mut Segment) {
        let segment_addr = segment.expose_provenance();
        debug_assert_eq!(
            segment_addr & !Self::PTR_MASK,
            0,
            "segment address does not fit in 48 bits"
        );
        let mut current = self.head.value.load(Ordering::Relaxed);
        loop {
            let current_addr = current & Self::PTR_MASK;
            let current_ptr = core::ptr::with_exposed_provenance_mut::<Segment>(current_addr);
            let next_count = ((current >> Self::PACKED_PTR_BITS) + 1) & Self::COUNT_WRAP_MASK;

            // Safety: segment pointer is valid, aligned, and exclusive to this thread.
            unsafe {
                (*segment).next_free_segment = current_ptr;
            }

            let next_val = (next_count << Self::PACKED_PTR_BITS) | segment_addr;

            match self.head.value.compare_exchange_weak(
                current,
                next_val,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    #[cfg(not(target_pointer_width = "64"))]
    #[inline]
    fn push_raw(&self, segment: *mut Segment) {
        let mut current = self.head.value.load(Ordering::Relaxed);
        loop {
            let next_ptr = current;
            // Safety: segment pointer is valid, aligned, and exclusive to this thread.
            unsafe {
                (*segment).next_free_segment = next_ptr;
            }
            match self.head.value.compare_exchange_weak(
                current,
                segment,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Pops a segment from the pool, if available.
    #[cfg(target_pointer_width = "64")]
    #[inline]
    pub fn pop(&self) -> Option<*mut Segment> {
        let mut current = self.head.value.load(Ordering::Acquire);
        loop {
            let current_addr = current & Self::PTR_MASK;
            if current_addr == 0 {
                return None;
            }
            let current_ptr = core::ptr::with_exposed_provenance_mut::<Segment>(current_addr);

            // Safety: current_ptr points to a valid Segment inside the pool.
            let next = unsafe { (*current_ptr).next_free_segment }.expose_provenance();
            let next_count = ((current >> Self::PACKED_PTR_BITS) + 1) & Self::COUNT_WRAP_MASK;
            let next_val = (next_count << Self::PACKED_PTR_BITS) | next;

            match self.head.value.compare_exchange_weak(
                current,
                next_val,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.retained.value.fetch_sub(1, Ordering::Relaxed);
                    return Some(current_ptr);
                }
                Err(actual) => current = actual,
            }
        }
    }

    #[cfg(not(target_pointer_width = "64"))]
    #[inline]
    pub fn pop(&self) -> Option<*mut Segment> {
        let mut current = self.head.value.load(Ordering::Acquire);
        loop {
            if current.is_null() {
                return None;
            }
            // Safety: current points to a valid Segment inside the pool. We load the next
            // pointer in the chain atomically.
            let next = unsafe { (*current).next_free_segment };
            match self.head.value.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.retained.value.fetch_sub(1, Ordering::Relaxed);
                    return Some(current);
                }
                Err(actual) => current = actual,
            }
        }
    }

    #[inline]
    pub fn retained_count(&self) -> usize {
        self.retained.value.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn purged_count(&self) -> usize {
        self.purged.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn purge_call_count(&self) -> usize {
        self.purge_calls.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn record_purge(&self, count: usize) {
        self.purge_calls.fetch_add(1, Ordering::Relaxed);
        self.purged.fetch_add(count, Ordering::Relaxed);
    }

    #[inline]
    pub fn reset_segments_count(&self) -> usize {
        self.reset_segments.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn reset_call_count(&self) -> usize {
        self.reset_calls.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn record_reset(&self, count: usize) {
        self.reset_calls.fetch_add(1, Ordering::Relaxed);
        self.reset_segments.fetch_add(count, Ordering::Relaxed);
    }
}

/// A NUMA-aware lock-free global pool of free segments partitioned by socket node.
pub struct GlobalSegmentPool {
    nodes: [NodeSegmentPool; 16],
}

impl GlobalSegmentPool {
    /// Creates a new empty `GlobalSegmentPool` with 16 NUMA node sub-pools.
    pub const fn new() -> Self {
        Self {
            nodes: [
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
            ],
        }
    }

    /// Pushes a segment back to the correct NUMA node pool without applying a retention limit.
    ///
    /// # Safety
    ///
    /// The `segment` pointer must be a valid, initialized, and exclusive pointer to a
    /// `Segment` structure.
    #[inline]
    pub unsafe fn push_unbounded(&self, segment: *mut Segment) {
        let node = unsafe { (*segment).numa_node } as usize % 16;
        self.nodes[node].push_unbounded(segment);
    }

    /// Pushes a segment back to its originating NUMA node pool if retention limit permits.
    ///
    /// # Safety
    ///
    /// The `segment` pointer must be a valid, initialized, and exclusive pointer to a
    /// `Segment` structure.
    #[inline]
    pub unsafe fn try_push_retained(&self, segment: *mut Segment) -> bool {
        let node = unsafe { (*segment).numa_node } as usize % 16;
        self.nodes[node].try_push_retained(segment)
    }

    /// Pops a segment from the calling thread's NUMA node pool, stealing from other nodes if empty.
    #[inline]
    pub fn pop(&self) -> Option<*mut Segment> {
        let mut node = current_numa_node() as usize % 16;
        // 1. Try local node first
        if let Some(segment) = self.nodes[node].pop() {
            return Some(segment);
        }
        // Local node cache miss: refresh our TLS cached NUMA node ID in case we migrated.
        let new_node = crate::numa::refresh_numa_node() as usize % 16;
        if new_node != node {
            node = new_node;
            if let Some(segment) = self.nodes[node].pop() {
                return Some(segment);
            }
        }
        // 2. Steal from other nodes
        for i in 1..16 {
            let other = (node + i) % 16;
            if let Some(segment) = self.nodes[other].pop() {
                return Some(segment);
            }
        }
        None
    }

    #[inline]
    pub fn retained_count(&self) -> usize {
        self.nodes.iter().map(|n| n.retained_count()).sum()
    }

    #[inline]
    pub fn purged_count(&self) -> usize {
        self.nodes.iter().map(|n| n.purged_count()).sum()
    }

    #[inline]
    pub fn purge_call_count(&self) -> usize {
        self.nodes.iter().map(|n| n.purge_call_count()).sum()
    }

    #[inline]
    pub(crate) fn record_purge(&self, count: usize) {
        let node = current_numa_node() as usize % 16;
        self.nodes[node].record_purge(count);
    }

    #[inline]
    pub fn reset_segments_count(&self) -> usize {
        self.nodes.iter().map(|n| n.reset_segments_count()).sum()
    }

    #[inline]
    pub fn reset_call_count(&self) -> usize {
        self.nodes.iter().map(|n| n.reset_call_count()).sum()
    }

    #[inline]
    pub(crate) fn record_reset(&self, count: usize) {
        let node = current_numa_node() as usize % 16;
        self.nodes[node].record_reset(count);
    }
}

impl Default for GlobalSegmentPool {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// A single lock-free size-bucket for cached huge allocations.
pub struct NodeHugeBucket {
    head: core::sync::atomic::AtomicPtr<Segment>,
    count: core::sync::atomic::AtomicUsize,
}

impl Default for NodeHugeBucket {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl NodeHugeBucket {
    /// Creates a new empty `NodeHugeBucket`.
    pub const fn new() -> Self {
        Self {
            head: core::sync::atomic::AtomicPtr::new(core::ptr::null_mut()),
            count: core::sync::atomic::AtomicUsize::new(0),
        }
    }
}

/// A lock-free pool of cached huge allocations for a single NUMA node, divided into 16 size-buckets.
pub struct NodeHugePool {
    buckets: [NodeHugeBucket; 16],
}

impl Default for NodeHugePool {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl NodeHugePool {
    /// Creates a new empty `NodeHugePool`.
    pub const fn new() -> Self {
        Self {
            buckets: [
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
            ],
        }
    }
}

/// A NUMA-aware lock-free global pool of free huge allocations.
pub struct GlobalHugePool {
    nodes: [NodeHugePool; 16],
}

#[inline(always)]
fn huge_bucket_index(size: usize) -> usize {
    let mb = size >> 20;
    if mb >= 16 {
        15
    } else {
        mb
    }
}

impl Default for GlobalHugePool {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalHugePool {
    /// Bounded maximum number of huge allocations cached per NUMA node bucket.
    pub const MAX_CACHED_HUGE_BLOCKS: usize = 1024;
    /// Maximum size class we cache (16MB).
    pub const MAX_CACHED_HUGE_SIZE: usize = 16 * 1024 * 1024;

    /// Creates a new empty `GlobalHugePool` with 16 NUMA node sub-pools.
    pub const fn new() -> Self {
        Self {
            nodes: [
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
            ],
        }
    }

    /// Pushes a free huge block segment back to the pool if space permits.
    ///
    /// # Safety
    ///
    /// `segment` must point to a valid, initialized, and exclusive `Segment` structure
    /// representing a huge allocation.
    pub unsafe fn try_push(&self, segment: *mut Segment, numa_node: usize) -> bool {
        let size = unsafe { (*segment).pages[0].block_size };
        if size > Self::MAX_CACHED_HUGE_SIZE {
            return false;
        }

        let node = numa_node % 16;
        let bucket_idx = huge_bucket_index(size);
        let pool_node = &self.nodes[node];
        let bucket = &pool_node.buckets[bucket_idx];

        let count = bucket.count.load(core::sync::atomic::Ordering::Relaxed);
        if count >= Self::MAX_CACHED_HUGE_BLOCKS {
            return false;
        }

        let mut head = bucket.head.load(core::sync::atomic::Ordering::Relaxed);
        loop {
            // Write next pointer into the segment header
            unsafe {
                (*segment).next_free_segment = head;
            }

            match bucket.head.compare_exchange_weak(
                head,
                segment,
                core::sync::atomic::Ordering::Release,
                core::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(_) => {
                    bucket
                        .count
                        .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    return true;
                }
                Err(actual) => head = actual,
            }
        }
    }

    /// Pops a huge block segment from the pool that is at least `size` bytes, stealing if needed.
    ///
    /// # Safety
    ///
    /// The returned segment is exclusively owned by the caller.
    pub unsafe fn pop<B: mnemosyne_core::MemoryBackend>(
        &self,
        size: usize,
        numa_node: usize,
    ) -> Option<*mut Segment> {
        let start_node = numa_node % 16;
        let bucket_idx = huge_bucket_index(size);

        // 1. Try local NUMA node first
        if let Some(res) = self.pop_from_node::<B>(size, start_node, bucket_idx) {
            return Some(res);
        }

        // 2. Steal from other nodes
        for i in 1..16 {
            let other_node = (start_node + i) % 16;
            if let Some(res) = self.pop_from_node::<B>(size, other_node, bucket_idx) {
                return Some(res);
            }
        }

        None
    }

    unsafe fn pop_from_node<B: mnemosyne_core::MemoryBackend>(
        &self,
        size: usize,
        node: usize,
        start_bucket: usize,
    ) -> Option<*mut Segment> {
        let pool_node = &self.nodes[node];
        for bucket_idx in start_bucket..16 {
            let bucket = &pool_node.buckets[bucket_idx];
            let mut head = bucket.head.load(core::sync::atomic::Ordering::Acquire);
            loop {
                if head.is_null() {
                    break;
                }

                let next = unsafe { (*head).next_free_segment };
                let block_size = unsafe { (*head).pages[0].block_size };

                if block_size >= size {
                    match bucket.head.compare_exchange_weak(
                        head,
                        next,
                        core::sync::atomic::Ordering::AcqRel,
                        core::sync::atomic::Ordering::Acquire,
                    ) {
                        Ok(_) => {
                            bucket
                                .count
                                .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                            return Some(head);
                        }
                        Err(actual) => head = actual,
                    }
                } else {
                    // If it is too small, pop it anyway, deallocate it to OS, and continue.
                    match bucket.head.compare_exchange_weak(
                        head,
                        next,
                        core::sync::atomic::Ordering::AcqRel,
                        core::sync::atomic::Ordering::Acquire,
                    ) {
                        Ok(_) => {
                            bucket
                                .count
                                .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                            let raw_ptr = unsafe { (*head).raw_alloc_ptr };
                            let _ = unsafe { B::deallocate(raw_ptr, block_size) };
                            head = bucket.head.load(core::sync::atomic::Ordering::Acquire);
                        }
                        Err(actual) => head = actual,
                    }
                }
            }
        }
        None
    }

    /// Purges all cached huge blocks and releases them to the OS.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the backend `B` is valid and that no threads
    /// are concurrently accessing the purged memory or segment pointers.
    pub unsafe fn purge<B: mnemosyne_core::MemoryBackend>(&self) {
        for node in 0..16 {
            let pool_node = &self.nodes[node];
            for bucket_idx in 0..16 {
                let bucket = &pool_node.buckets[bucket_idx];
                let mut head = bucket
                    .head
                    .swap(core::ptr::null_mut(), core::sync::atomic::Ordering::Acquire);
                bucket.count.store(0, core::sync::atomic::Ordering::Relaxed);

                while !head.is_null() {
                    let next = unsafe { (*head).next_free_segment };
                    let raw_ptr = unsafe { (*head).raw_alloc_ptr };
                    let block_size = unsafe { (*head).pages[0].block_size };
                    let _ = unsafe { B::deallocate(raw_ptr, block_size) };
                    head = next;
                }
            }
        }
    }
}

/// Sealed trait module to protect architectural invariants.
#[doc(hidden)]
pub mod private {
    pub trait Sealed {}
}

/// Trait associating a memory backend with its global pools.
pub trait HasSegmentPool: mnemosyne_core::MemoryBackend + private::Sealed {
    /// Returns the global segment pool for this backend.
    fn global_segment_pool() -> &'static GlobalSegmentPool;

    /// Returns the global orphan pool for this backend.
    fn global_orphan_pool() -> &'static GlobalSegmentPool;

    /// Returns the global huge allocation pool for this backend.
    fn global_huge_pool() -> &'static GlobalHugePool;
}

static DEFAULT_BACKEND_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static DEFAULT_BACKEND_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static DEFAULT_BACKEND_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();

impl private::Sealed for mnemosyne_backend::DefaultBackend {}

impl HasSegmentPool for mnemosyne_backend::DefaultBackend {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &DEFAULT_BACKEND_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &DEFAULT_BACKEND_ORPHAN_POOL
    }

    #[inline(always)]
    fn global_huge_pool() -> &'static GlobalHugePool {
        &DEFAULT_BACKEND_HUGE_POOL
    }
}

static WRAPPER_BACKEND_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static WRAPPER_BACKEND_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static WRAPPER_BACKEND_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();

impl private::Sealed for mnemosyne_backend::MemoryBackendWrapper {}

impl HasSegmentPool for mnemosyne_backend::MemoryBackendWrapper {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &WRAPPER_BACKEND_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &WRAPPER_BACKEND_ORPHAN_POOL
    }

    #[inline(always)]
    fn global_huge_pool() -> &'static GlobalHugePool {
        &WRAPPER_BACKEND_HUGE_POOL
    }
}

static CUDA_BACKEND_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static CUDA_BACKEND_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static CUDA_BACKEND_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();

impl private::Sealed for mnemosyne_backend::CudaUnifiedBackend {}

impl HasSegmentPool for mnemosyne_backend::CudaUnifiedBackend {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &CUDA_BACKEND_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &CUDA_BACKEND_ORPHAN_POOL
    }

    #[inline(always)]
    fn global_huge_pool() -> &'static GlobalHugePool {
        &CUDA_BACKEND_HUGE_POOL
    }
}
