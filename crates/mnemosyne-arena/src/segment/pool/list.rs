//! Lock-free per-NUMA-node segment pool (Treiber stack).
//!
//! The pool is a singly-linked stack of free [`Segment`] nodes whose head
//! pointer is an [`AtomicPtr`]. Push and pop are lock-free CAS loops;
//! [`take_all`] is a single atomic swap. No spinlock or mutex is acquired
//! on any path.
//!
//! [`take_all`]: NodeSegmentPool::take_all

use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use mnemosyne_core::types::Segment;

/// Cache-line aligned atomic counter to avoid false sharing between
/// adjacent per-node pools.
#[repr(align(64))]
pub(crate) struct CacheAlignedAtomicUsize {
    pub(crate) value: AtomicUsize,
}

impl CacheAlignedAtomicUsize {
    #[inline(always)]
    pub(crate) const fn new(val: usize) -> Self {
        Self {
            value: AtomicUsize::new(val),
        }
    }
}

/// A lock-free segment pool for a single NUMA node (Treiber stack).
///
/// The pool maintains a singly-linked stack of free [`Segment`] pointers
/// through the segment's `next_free_segment` field. All operations are
/// lock-free: push and pop use CAS loops on the head atomic, and
/// [`take_all`] uses a single atomic swap.
#[repr(align(64))]
pub struct NodeSegmentPool {
    /// Head of the Treiber stack (null when empty).
    head: AtomicPtr<Segment>,
    /// Advisory count of segments currently in the pool.
    retained: CacheAlignedAtomicUsize,
    purged: AtomicUsize,
    purge_calls: AtomicUsize,
    reset_segments: AtomicUsize,
    reset_calls: AtomicUsize,
}

// SAFETY: The `head` atomic pointer is accessed via CAS loops that ensure
// only one thread mutates a given segment's `next_free_segment` at a time
// (the producer writes `next` before the CAS publishes the segment; the
// consumer reads `next` after a successful CAS claims the segment). The
// telemetry counters are independently synchronized atomics. This discipline
// makes shared cross-thread access (`Sync`) and ownership transfer (`Send`)
// of a `NodeSegmentPool` data-race free.
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
            head: AtomicPtr::new(core::ptr::null_mut()),
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
        // SAFETY: by this function's contract `segment` is a valid, exclusively
        // owned `Segment`. We write `next_free_segment` before publishing via
        // CAS, so no other thread can observe a stale `next` value.
        unsafe {
            let mut current = self.head.load(Ordering::Relaxed);
            loop {
                (*segment).next_free_segment = current;
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
        self.retained.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Pushes a segment back to the bounded reusable segment pool.
    ///
    /// Returns `true` if the segment was successfully cached, or `false` if
    /// the pool is already full.
    ///
    /// # Safety
    ///
    /// The `segment` pointer must be a valid, initialized, and exclusive pointer to a
    /// `Segment` structure. The caller must transfer ownership of that segment back to
    /// the pool.
    #[inline]
    pub unsafe fn try_push_retained(&self, segment: *mut Segment) -> bool {
        // Soft limit check: the retained count is advisory, so a small overshoot
        // under contention is acceptable for a cache. The alternative — holding
        // a lock to atomically check-and-push — is the contention source we are
        // eliminating.
        let retained = self.retained.value.load(Ordering::Relaxed);
        if retained >= mnemosyne_core::options::MAX_RETAINED_SEGMENTS.load(Ordering::Relaxed) {
            return false;
        }
        // SAFETY: same contract as `push_unbounded`.
        unsafe {
            let mut current = self.head.load(Ordering::Relaxed);
            loop {
                (*segment).next_free_segment = current;
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
        self.retained.value.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Detaches the entire retained chain in a single atomic swap, returning
    /// its head (or null) and the number of segments detached, and leaving
    /// the pool empty.
    ///
    /// This is a single `swap` on the head atomic — no lock acquisition, so
    /// it never serializes with allocators pushing/popping on the same pool.
    /// Ownership of every detached segment transfers to the caller, which
    /// must release or re-cache them.
    #[inline]
    pub fn take_all(&self) -> (*mut Segment, usize) {
        let head = self.head.swap(core::ptr::null_mut(), Ordering::Acquire);
        // `retained` is maintained equal to the chain length, so the
        // swapped-out value is the detached count.
        let count = self.retained.value.swap(0, Ordering::Relaxed);
        (head, count)
    }

    /// Pops a segment from the pool, if available.
    #[inline]
    pub fn pop(&self) -> Option<*mut Segment> {
        // Fast path: if the advisory counter says empty, skip the CAS loop.
        if self.retained.value.load(Ordering::Relaxed) == 0 {
            return None;
        }
        let mut current = self.head.load(Ordering::Acquire);
        loop {
            if current.is_null() {
                return None;
            }
            // SAFETY: `current` is a non-null pointer loaded from the head
            // atomic. Between our load and our CAS, no other consumer can
            // pop this same segment (they would see the same head and
            // compete on the CAS). A producer might push a new segment on
            // top, but that changes `head`, causing our CAS to fail and
            // retry — it does not invalidate `current`. Reading
            // `next_free_segment` is safe because the producer that pushed
            // `current` wrote `next` before publishing via CAS.
            let next = unsafe { (*current).next_free_segment };
            match self.head.compare_exchange_weak(
                current,
                next,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.retained.value.fetch_sub(1, Ordering::Relaxed);
                    // Clear the next pointer so the returned segment is clean.
                    // SAFETY: we now exclusively own `current` (CAS succeeded,
                    // so no other thread can access it through the pool).
                    unsafe { (*current).next_free_segment = core::ptr::null_mut() };
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
