use super::cache_aligned::{CacheAlignedAtomicPtr, CacheAlignedAtomicUsize};
use core::sync::atomic::Ordering;
use mnemosyne_core::types::Segment;
use themis::NumaNodeId;

const NUMA_BUCKETS: usize = 16;
const HUGE_SIZE_BUCKETS: usize = 16;

#[inline(always)]
fn numa_bucket(node: usize) -> usize {
    NumaNodeId::new(node as u32)
        .bucket_index::<NUMA_BUCKETS>()
        .index()
}

/// A size-bucket for cached huge allocations.
pub struct NodeHugeBucket {
    /// Head of the Treiber stack (null when empty).
    head: CacheAlignedAtomicPtr,
    /// Advisory count of segments currently retained in this bucket.
    count: CacheAlignedAtomicUsize,
}

// SAFETY: the `head` atomic pointer is mutated only by CAS loops. A producer
// writes a segment's `next_free_segment` before publishing it with `Release`;
// a consumer reads the link after an `Acquire` load/CAS and clears it only
// after a successful CAS gives it exclusive ownership. The count atomic is
// advisory metadata and is independently synchronized.
unsafe impl Send for NodeHugeBucket {}
unsafe impl Sync for NodeHugeBucket {}

impl NodeHugeBucket {
    /// Creates a new empty `NodeHugeBucket`.
    pub const fn new() -> Self {
        Self {
            head: CacheAlignedAtomicPtr::new(core::ptr::null_mut()),
            count: CacheAlignedAtomicUsize::new(0),
        }
    }

    #[inline(always)]
    fn count(&self) -> usize {
        self.count.value.load(Ordering::Relaxed)
    }

    /// Pushes a segment onto this bucket's Treiber stack.
    ///
    /// # Safety
    ///
    /// `segment` must be a valid, initialized, and exclusively owned huge
    /// allocation segment. Ownership transfers to this bucket on success.
    #[inline]
    unsafe fn push(&self, segment: *mut Segment) {
        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            let current_ptr = CacheAlignedAtomicPtr::ptr(current);
            // SAFETY: by this function's contract, the caller owns `segment`
            // exclusively until the successful CAS publishes it.
            unsafe {
                (*segment).next_free_segment = current_ptr;
            }
            let next = CacheAlignedAtomicPtr::tagged_successor(segment, current);
            match self.head.compare_exchange_weak(
                current,
                next,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
        self.count.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Pops the head segment from this bucket, if any.
    #[inline]
    fn pop_head(&self) -> Option<*mut Segment> {
        if self.count() == 0 {
            return None;
        }

        let mut current = self.head.load(Ordering::Acquire);
        loop {
            let current_ptr = CacheAlignedAtomicPtr::ptr(current);
            if current_ptr.is_null() {
                return None;
            }

            // SAFETY: `current` was read from the stack head. The producer
            // wrote `next_free_segment` before publishing with `Release`; the
            // `Acquire` load/CAS observes that initialized link. If another
            // thread changes the head, our CAS fails and this read is retried
            // against the current head.
            let next_ptr = unsafe { (*current_ptr).next_free_segment };
            let next = CacheAlignedAtomicPtr::tagged_successor(next_ptr, current);
            match self.head.compare_exchange_weak(
                current,
                next,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.count.value.fetch_sub(1, Ordering::Relaxed);
                    // SAFETY: the successful CAS removed `current` from the
                    // shared stack, so this thread now owns the segment.
                    unsafe {
                        (*current_ptr).next_free_segment = core::ptr::null_mut();
                    }
                    return Some(current_ptr);
                }
                Err(actual) => current = actual,
            }
        }
    }

    /// Detaches this bucket's full retained chain in one atomic operation.
    #[inline]
    fn take_all(&self) -> (*mut Segment, usize) {
        let head = self.head.swap_null(Ordering::Acquire);
        let count = self.count.value.swap(0, Ordering::Relaxed);
        (CacheAlignedAtomicPtr::ptr(head), count)
    }
}

impl Default for NodeHugeBucket {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// A lock-free pool of cached huge allocations for a single NUMA node.
pub struct NodeHugePool {
    pub(crate) buckets: [NodeHugeBucket; HUGE_SIZE_BUCKETS],
    pub(crate) total_count: CacheAlignedAtomicUsize,
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
            buckets: [const { NodeHugeBucket::new() }; HUGE_SIZE_BUCKETS],
            total_count: CacheAlignedAtomicUsize::new(0),
        }
    }
}

/// A NUMA-aware lock-free global pool of free huge allocations.
pub struct GlobalHugePool {
    nodes: [NodeHugePool; NUMA_BUCKETS],
}

#[inline(always)]
pub(crate) fn huge_bucket_index(size: usize) -> usize {
    if size <= 16384 {
        0
    } else {
        let bits = usize::BITS - (size - 1).leading_zeros();
        let idx = (bits as usize).saturating_sub(14);
        if idx >= HUGE_SIZE_BUCKETS {
            HUGE_SIZE_BUCKETS - 1
        } else {
            idx
        }
    }
}

/// Per-bucket retained-block cap, derived from a per-bucket byte budget.
///
/// A flat `MAX_CACHED_HUGE_BLOCKS` count cap lets a large-size bucket retain
/// `1024 × (block size)` bytes — up to ~16 GiB for the 16 MiB bucket. Capping
/// each bucket's *retained bytes* instead (block count ≤ budget / max block size
/// in the bucket) bounds idle RSS while leaving the small-huge buckets at the
/// full count cap, so the warm cache for the common case is preserved. Bucket
/// `b` holds sizes in `(2^(b+13), 2^(b+14)]`, so `2^(b+14)` upper-bounds a block.
#[inline(always)]
pub(crate) const fn bucket_block_cap(bucket_idx: usize) -> usize {
    let max_block = 1usize << (bucket_idx + 14);
    let cap = GlobalHugePool::MAX_CACHED_HUGE_BYTES_PER_BUCKET / max_block;
    if cap == 0 {
        1
    } else if cap > GlobalHugePool::MAX_CACHED_HUGE_BLOCKS {
        GlobalHugePool::MAX_CACHED_HUGE_BLOCKS
    } else {
        cap
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
    /// Per-bucket retained-byte budget. The effective per-bucket block cap is
    /// `min(MAX_CACHED_HUGE_BLOCKS, this / max-block-size-in-bucket)`, so a single
    /// node bucket never retains more than ~256 MiB of idle huge mappings (vs. up
    /// to ~16 GiB under a flat count cap). The private `bucket_block_cap`
    /// helper derives the effective per-bucket limit.
    pub const MAX_CACHED_HUGE_BYTES_PER_BUCKET: usize = 256 * 1024 * 1024;

    /// Creates a new empty `GlobalHugePool` with `NUMA_BUCKETS` node sub-pools.
    pub const fn new() -> Self {
        // Derive the array length from the `NUMA_BUCKETS` SSOT rather than a
        // hand-written literal, so the fan-out can never drift from the constant.
        Self {
            nodes: [const { NodeHugePool::new() }; NUMA_BUCKETS],
        }
    }

    /// Pushes a free huge block segment back to the pool if space permits.
    ///
    /// # Safety
    ///
    /// `segment` must point to a valid, initialized, and exclusive `Segment` structure
    /// representing a huge allocation.
    #[inline]
    pub unsafe fn try_push(&self, segment: *mut Segment, numa_node: usize) -> bool {
        // SAFETY: by this function's contract `segment` is a valid, initialized,
        // exclusively-owned huge-allocation `Segment`, so reading its page-0
        // `block_size` is sound.
        let size = unsafe { (*segment).pages[0].block_size };
        if size > Self::MAX_CACHED_HUGE_SIZE {
            return false;
        }

        let node = numa_bucket(numa_node);
        let bucket_idx = huge_bucket_index(size);
        let pool_node = &self.nodes[node];
        let bucket = &pool_node.buckets[bucket_idx];

        if bucket.count() >= bucket_block_cap(bucket_idx) {
            return false;
        }

        // SAFETY: by this function's contract, ownership of `segment`
        // transfers to the pool on a successful cache insertion.
        unsafe {
            bucket.push(segment);
        }

        pool_node
            .total_count
            .value
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        true
    }

    /// Pops a huge block segment from the pool that is at least `size` bytes, stealing if needed.
    ///
    /// # Safety
    ///
    /// The returned segment is exclusively owned by the caller.
    #[inline]
    pub unsafe fn pop(&self, size: usize, numa_node: usize) -> Option<*mut Segment> {
        let start_node = numa_bucket(numa_node);
        let bucket_idx = huge_bucket_index(size);

        // `pop_from_node` already early-returns on an empty node (its leading
        // `total_count == 0` check), so a redundant pre-load here would only
        // re-read the same atomic. Call it directly: local node first, then steal.
        if let Some(res) = self.pop_from_node(size, start_node, bucket_idx) {
            return Some(res);
        }

        let start = NumaNodeId::new(start_node as u32).bucket_index::<NUMA_BUCKETS>();
        for i in 1..NUMA_BUCKETS {
            let other_node = start.wrapping_add(i).index();
            if let Some(res) = self.pop_from_node(size, other_node, bucket_idx) {
                return Some(res);
            }
        }

        None
    }

    #[inline]
    unsafe fn pop_from_node(
        &self,
        size: usize,
        node: usize,
        start_bucket: usize,
    ) -> Option<*mut Segment> {
        let pool_node = &self.nodes[node];
        if pool_node
            .total_count
            .value
            .load(core::sync::atomic::Ordering::Relaxed)
            == 0
        {
            return None;
        }

        for bucket_idx in start_bucket..HUGE_SIZE_BUCKETS {
            let bucket = &pool_node.buckets[bucket_idx];
            if bucket.count() == 0 {
                continue;
            }

            let popped = if bucket_idx == start_bucket {
                // SAFETY: this method owns each temporarily detached segment
                // until it either returns a fit or restores the rejected chain.
                unsafe { Self::pop_fitting_from_exact_bucket(bucket, size) }
            } else {
                // Higher bucket: every retained block is at least `size`.
                bucket.pop_head()
            };

            if let Some(segment) = popped {
                pool_node
                    .total_count
                    .value
                    .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                return Some(segment);
            }
        }
        None
    }

    #[inline]
    unsafe fn pop_fitting_from_exact_bucket(
        bucket: &NodeHugeBucket,
        size: usize,
    ) -> Option<*mut Segment> {
        let mut rejected_head: *mut Segment = core::ptr::null_mut();
        while let Some(segment) = bucket.pop_head() {
            // SAFETY: `pop_head` transfers exclusive ownership of `segment`.
            let block_size = unsafe { (*segment).pages[0].block_size };
            if block_size >= size {
                // SAFETY: every rejected segment was removed from the shared
                // stack and linked only through `rejected_head`.
                unsafe {
                    Self::restore_rejected(bucket, rejected_head);
                }
                return Some(segment);
            }

            // SAFETY: `segment` is exclusively owned and not reachable from a
            // shared pool until `restore_rejected` republishes it.
            unsafe {
                (*segment).next_free_segment = rejected_head;
            }
            rejected_head = segment;
        }

        // SAFETY: every rejected segment was removed from the shared stack and
        // linked only through `rejected_head`.
        unsafe {
            Self::restore_rejected(bucket, rejected_head);
        }
        None
    }

    #[inline]
    unsafe fn restore_rejected(bucket: &NodeHugeBucket, mut head: *mut Segment) {
        while !head.is_null() {
            // SAFETY: `head` is part of a private rejected chain owned by this
            // caller. Capture `next` before publishing `head` back to the
            // shared Treiber stack.
            let next = unsafe { (*head).next_free_segment };
            // SAFETY: ownership of this rejected segment transfers back to the
            // bucket.
            unsafe {
                bucket.push(head);
            }
            head = next;
        }
    }

    /// Purges all cached huge blocks and releases them to the OS.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the backend `B` is valid and that no threads
    /// are concurrently accessing the purged memory or segment pointers.
    pub unsafe fn purge<B: mnemosyne_core::MemoryBackend>(&self) {
        for node in 0..NUMA_BUCKETS {
            let pool_node = &self.nodes[node];
            for bucket_idx in 0..HUGE_SIZE_BUCKETS {
                let bucket = &pool_node.buckets[bucket_idx];
                let (mut head, count) = bucket.take_all();
                if count == 0 {
                    continue;
                }
                pool_node
                    .total_count
                    .value
                    .fetch_sub(count, core::sync::atomic::Ordering::Relaxed);

                while !head.is_null() {
                    // SAFETY: `head` is a segment detached from this bucket by
                    // `take_all` and is no longer reachable by any other thread
                    // (the caller guarantees no concurrent access during purge),
                    // so it is exclusively owned here. Reading its links/size and
                    // releasing its recorded mapping through the allocating
                    // backend `B` is sound; `next` is captured before the mapping
                    // is freed.
                    let next = unsafe {
                        let next = (*head).next_free_segment;
                        let raw_ptr = (*head).raw_alloc_ptr;
                        let block_size = (*head).pages[0].block_size;
                        let _ = B::deallocate(raw_ptr, block_size);
                        next
                    };
                    head = next;
                }
            }
        }
    }
}
