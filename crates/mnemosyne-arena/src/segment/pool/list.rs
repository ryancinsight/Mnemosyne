use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::types::Segment;

#[repr(align(64))]
pub(crate) struct CacheAlignedAtomicUsize {
    pub(crate) value: core::sync::atomic::AtomicUsize,
}

impl CacheAlignedAtomicUsize {
    #[inline(always)]
    pub(crate) const fn new(val: usize) -> Self {
        Self {
            value: core::sync::atomic::AtomicUsize::new(val),
        }
    }
}

#[cfg(not(target_pointer_width = "64"))]
#[repr(align(64))]
pub(crate) struct CacheAlignedAtomicPtr<T> {
    pub(crate) value: core::sync::atomic::AtomicPtr<T>,
}

#[cfg(not(target_pointer_width = "64"))]
impl<T> CacheAlignedAtomicPtr<T> {
    #[inline(always)]
    pub(crate) const fn new(val: *mut T) -> Self {
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
