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

struct NodeSegmentPoolState {
    head: *mut Segment,
}

/// A segment pool for a single NUMA node.
#[repr(align(64))]
pub struct NodeSegmentPool {
    lock: mnemosyne_core::sync::SpinLock,
    state: core::cell::UnsafeCell<NodeSegmentPoolState>,
    retained: CacheAlignedAtomicUsize,
    purged: core::sync::atomic::AtomicUsize,
    purge_calls: core::sync::atomic::AtomicUsize,
    reset_segments: core::sync::atomic::AtomicUsize,
    reset_calls: core::sync::atomic::AtomicUsize,
}

unsafe impl Send for NodeSegmentPool {}
unsafe impl Sync for NodeSegmentPool {}

impl Default for NodeSegmentPool {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl NodeSegmentPool {
    /// Creates a new empty `NodeSegmentPool`.
    pub const fn new() -> Self {
        Self {
            lock: mnemosyne_core::sync::SpinLock::new(),
            state: core::cell::UnsafeCell::new(NodeSegmentPoolState {
                head: core::ptr::null_mut(),
            }),
            retained: CacheAlignedAtomicUsize::new(0),
            purged: AtomicUsize::new(0),
            purge_calls: AtomicUsize::new(0),
            reset_segments: AtomicUsize::new(0),
            reset_calls: AtomicUsize::new(0),
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
        self.lock.lock();
        // Safety: We hold the spinlock, so we have exclusive access to the state.
        unsafe {
            let state = &mut *self.state.get();
            (*segment).next_free_segment = state.head;
            state.head = segment;
        }
        self.retained.value.fetch_add(1, Ordering::Relaxed);
        self.lock.unlock();
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
        self.lock.lock();
        let retained = self.retained.value.load(Ordering::Relaxed);
        if retained >= mnemosyne_core::options::MAX_RETAINED_SEGMENTS.load(Ordering::Relaxed) {
            self.lock.unlock();
            return false;
        }
        // Safety: We hold the spinlock, so we have exclusive access to the state.
        unsafe {
            let state = &mut *self.state.get();
            (*segment).next_free_segment = state.head;
            state.head = segment;
        }
        self.retained.value.store(retained + 1, Ordering::Relaxed);
        self.lock.unlock();
        true
    }

    /// Pops a segment from the pool, if available.
    #[inline]
    pub fn pop(&self) -> Option<*mut Segment> {
        self.lock.lock();
        // Safety: We hold the spinlock, so we have exclusive access to the state.
        let segment = unsafe {
            let state = &mut *self.state.get();
            let segment = state.head;
            if !segment.is_null() {
                state.head = (*segment).next_free_segment;
                (*segment).next_free_segment = core::ptr::null_mut();
                self.retained.value.fetch_sub(1, Ordering::Relaxed);
                Some(segment)
            } else {
                None
            }
        };
        self.lock.unlock();
        segment
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
