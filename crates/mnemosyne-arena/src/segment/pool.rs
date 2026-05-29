//! Global segment pool management.

use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::types::Segment;
use super::alloc::MAX_RETAINED_SEGMENTS;

/// A lock-free global pool of free segments to avoid OS allocator overhead.
#[cfg(target_pointer_width = "64")]
pub struct GlobalSegmentPool {
    head: core::sync::atomic::AtomicUsize,
    retained: core::sync::atomic::AtomicUsize,
    purged: core::sync::atomic::AtomicUsize,
    purge_calls: core::sync::atomic::AtomicUsize,
    reset_segments: core::sync::atomic::AtomicUsize,
    reset_calls: core::sync::atomic::AtomicUsize,
}

#[cfg(not(target_pointer_width = "64"))]
pub struct GlobalSegmentPool {
    head: core::sync::atomic::AtomicPtr<Segment>,
    retained: core::sync::atomic::AtomicUsize,
    purged: core::sync::atomic::AtomicUsize,
    purge_calls: core::sync::atomic::AtomicUsize,
    reset_segments: core::sync::atomic::AtomicUsize,
    reset_calls: core::sync::atomic::AtomicUsize,
}

#[cfg(target_pointer_width = "64")]
impl GlobalSegmentPool {
    /// Low bits reserved for the packed segment address.
    const PACKED_PTR_BITS: u32 = 48;
    /// Mask selecting the packed address bits.
    const PTR_MASK: usize = (1usize << Self::PACKED_PTR_BITS) - 1;
    /// Mask wrapping the push counter to the remaining high bits.
    const COUNT_WRAP_MASK: usize = (1usize << (usize::BITS - Self::PACKED_PTR_BITS)) - 1;
}

impl GlobalSegmentPool {
    /// Creates a new empty `GlobalSegmentPool`.
    pub const fn new() -> Self {
        #[cfg(target_pointer_width = "64")]
        {
            Self {
                head: core::sync::atomic::AtomicUsize::new(0),
                retained: AtomicUsize::new(0),
                purged: AtomicUsize::new(0),
                purge_calls: AtomicUsize::new(0),
                reset_segments: AtomicUsize::new(0),
                reset_calls: AtomicUsize::new(0),
            }
        }
        #[cfg(not(target_pointer_width = "64"))]
        {
            Self {
                head: core::sync::atomic::AtomicPtr::new(core::ptr::null_mut()),
                retained: AtomicUsize::new(0),
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
        self.retained.fetch_add(1, Ordering::Relaxed);
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
        let mut retained = self.retained.load(Ordering::Relaxed);
        loop {
            if retained >= MAX_RETAINED_SEGMENTS {
                return false;
            }
            match self.retained.compare_exchange_weak(
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
        let segment_addr = segment as usize;
        debug_assert_eq!(
            segment_addr & !Self::PTR_MASK,
            0,
            "segment address does not fit in 48 bits"
        );
        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            let current_addr = current & Self::PTR_MASK;
            let current_ptr = current_addr as *mut Segment;
            let next_count = ((current >> Self::PACKED_PTR_BITS) + 1) & Self::COUNT_WRAP_MASK;

            // Safety: segment pointer is valid, aligned, and exclusive to this thread.
            unsafe {
                (*segment).next_free_segment = current_ptr;
            }

            let next_val = (next_count << Self::PACKED_PTR_BITS) | segment_addr;

            match self.head.compare_exchange_weak(
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
        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            let next_ptr = current;
            // Safety: segment pointer is valid, aligned, and exclusive to this thread.
            unsafe {
                (*segment).next_free_segment = next_ptr;
            }
            match self.head.compare_exchange_weak(
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
        let mut current = self.head.load(Ordering::Acquire);
        loop {
            let current_addr = current & Self::PTR_MASK;
            if current_addr == 0 {
                return None;
            }
            let current_ptr = current_addr as *mut Segment;

            // Safety: current_ptr points to a valid Segment inside the pool.
            let next = unsafe { (*current_ptr).next_free_segment } as usize;
            let next_count = ((current >> Self::PACKED_PTR_BITS) + 1) & Self::COUNT_WRAP_MASK;
            let next_val = (next_count << Self::PACKED_PTR_BITS) | next;

            match self.head.compare_exchange_weak(
                current,
                next_val,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.retained.fetch_sub(1, Ordering::Relaxed);
                    return Some(current_ptr);
                }
                Err(actual) => current = actual,
            }
        }
    }

    #[cfg(not(target_pointer_width = "64"))]
    #[inline]
    pub fn pop(&self) -> Option<*mut Segment> {
        let mut current = self.head.load(Ordering::Acquire);
        loop {
            if current.is_null() {
                return None;
            }
            // Safety: current points to a valid Segment inside the pool. We load the next
            // pointer in the chain atomically.
            let next = unsafe { (*current).next_free_segment };
            match self.head.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.retained.fetch_sub(1, Ordering::Relaxed);
                    return Some(current);
                }
                Err(actual) => current = actual,
            }
        }
    }

    #[inline]
    pub fn retained_count(&self) -> usize {
        self.retained.load(Ordering::Relaxed)
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

/// Trait associating a memory backend with its global segment and orphan pools.
pub trait HasSegmentPool: mnemosyne_core::MemoryBackend {
    /// Returns the global segment pool for this backend.
    fn global_segment_pool() -> &'static GlobalSegmentPool;

    /// Returns the global orphan pool for this backend.
    fn global_orphan_pool() -> &'static GlobalSegmentPool;
}

static DEFAULT_BACKEND_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static DEFAULT_BACKEND_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();

impl HasSegmentPool for mnemosyne_backend::DefaultBackend {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &DEFAULT_BACKEND_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &DEFAULT_BACKEND_ORPHAN_POOL
    }
}

static WRAPPER_BACKEND_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static WRAPPER_BACKEND_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();

impl HasSegmentPool for mnemosyne_backend::MemoryBackendWrapper {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &WRAPPER_BACKEND_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &WRAPPER_BACKEND_ORPHAN_POOL
    }
}

static CUDA_BACKEND_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static CUDA_BACKEND_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();

impl HasSegmentPool for mnemosyne_backend::CudaUnifiedBackend {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &CUDA_BACKEND_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &CUDA_BACKEND_ORPHAN_POOL
    }
}
