//! Reclamation-safe per-NUMA-node segment pool.
//!
//! The pool is a singly-linked stack of free [`Segment`] nodes whose head is a
//! tagged `TaggedHead` (address + wrapping mutation tag). The shared
//! stack serializes head observation through successor access or detachment so
//! a decay sweep may release a detached mapping after `take_all` returns.
//!
use super::tagged_stack::TaggedSegmentStack;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::types::Segment;

/// A reclamation-safe segment pool for a single NUMA node.
///
/// The free-segment stack and its ABA-tag / ordering discipline live in the
/// `TaggedSegmentStack`; this type layers the per-pool telemetry counters and
/// the retention cap on top. `Send`/`Sync` are compiler-derived (the struct
/// holds only atomics).
#[repr(align(64))]
pub struct NodeSegmentPool {
    /// Reclamation-safe stack of free segments plus its retained count.
    stack: TaggedSegmentStack,
    purged: AtomicUsize,
    purge_calls: AtomicUsize,
    reset_segments: AtomicUsize,
    reset_calls: AtomicUsize,
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
        Self {
            stack: TaggedSegmentStack::new(),
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
        // SAFETY: forwarded contract — `segment` is an exclusively-owned
        // `Segment` whose ownership transfers to the stack.
        unsafe { self.stack.push(segment) };
    }

    /// Pushes a pre-linked chain back to the pool in one lock acquisition,
    /// without applying a retention limit.
    ///
    /// The batched form of [`Self::push_unbounded`]: a caller returning many
    /// segments at once — thread-exit reclamation is the motivating one — pays
    /// one acquisition instead of one per segment.
    ///
    /// # Safety
    ///
    /// The backing stack's `push_chain` contract applies: `head` and `tail`
    /// must be exclusively-owned `Segment`s linked through
    /// `next_free_segment` with `tail` exactly `len - 1` hops from `head`
    /// (`len >= 1`). Ownership of every chain node transfers to the pool.
    #[inline]
    pub unsafe fn push_chain_unbounded(&self, head: *mut Segment, tail: *mut Segment, len: usize) {
        // SAFETY: forwarded contract — an exclusively-owned chain whose
        // ownership transfers to the stack.
        unsafe { self.stack.push_chain(head, tail, len) };
    }

    /// [`Self::push_chain_unbounded`] for a caller that must not wait on the
    /// stack's lifetime lock, such as a destructor.
    ///
    /// Returns whether the chain was pushed; on `false` the caller retains
    /// ownership of every node.
    ///
    /// # Safety
    ///
    /// As [`Self::push_chain_unbounded`]; ownership transfers only when this
    /// returns `true`.
    #[inline]
    pub unsafe fn try_push_chain_unbounded(
        &self,
        head: *mut Segment,
        tail: *mut Segment,
        len: usize,
    ) -> bool {
        // SAFETY: forwarded contract — as `push_chain_unbounded`.
        unsafe { self.stack.try_push_chain(head, tail, len) }
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
        let retained = self.stack.len();
        if retained >= mnemosyne_core::options::MAX_RETAINED_SEGMENTS.load(Ordering::Relaxed) {
            return false;
        }
        // SAFETY: same contract as `push_unbounded` — `segment` is an
        // exclusively-owned `Segment` transferring ownership to the stack.
        unsafe { self.stack.push(segment) };
        true
    }

    /// [`Self::try_push_retained`] for a caller that must not wait on the
    /// stack's lifetime lock, such as a destructor.
    ///
    /// Returns `false` both when the retention cap is reached and when the lock
    /// is busy; the two are indistinguishable to the caller because the
    /// response is the same — dispose of the segment another way. On `false`
    /// the caller retains ownership.
    ///
    /// # Safety
    ///
    /// As [`Self::try_push_retained`]; ownership transfers only when this
    /// returns `true`.
    #[inline]
    pub unsafe fn try_push_retained_without_waiting(&self, segment: *mut Segment) -> bool {
        let retained = self.stack.len();
        if retained >= mnemosyne_core::options::MAX_RETAINED_SEGMENTS.load(Ordering::Relaxed) {
            return false;
        }
        // SAFETY: same contract as `try_push_retained` — `segment` is an
        // exclusively-owned `Segment` transferring ownership to the stack.
        unsafe { self.stack.try_push(segment) }
    }

    /// Detaches the entire retained chain in a single atomic swap, returning
    /// its head (or null) and the number of segments detached, and leaving
    /// the pool empty.
    ///
    /// The stack's lifetime lock protects the head and intrusive links while
    /// detaching; the returned chain is therefore no longer reachable by
    /// concurrent pool operations when ownership transfers to the caller.
    /// Ownership of every detached segment transfers to the caller, which
    /// must release or re-cache them.
    #[inline]
    pub fn take_all(&self) -> (*mut Segment, usize) {
        self.stack.take_all()
    }

    /// Pops a segment from the pool, if available.
    #[inline]
    pub fn pop(&self) -> Option<*mut Segment> {
        let popped = self.stack.pop();
        if popped.is_null() { None } else { Some(popped) }
    }

    #[inline]
    pub fn retained_count(&self) -> usize {
        self.stack.len()
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
