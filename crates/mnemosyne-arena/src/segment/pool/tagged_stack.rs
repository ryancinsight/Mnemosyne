//! Reclamation-safe intrusive stack of [`Segment`]s with an advisory retained
//! count — the single authoritative implementation of the tagged-pointer head
//! shared by the huge-allocation cache
//! ([`super::huge_pool`]) and the segment pool ([`super::list`]).
//!
//! Both pools previously hand-drove the identical push / pop / `take_all` CAS
//! loops over [`TaggedHead`]; centralizing them here means the head
//! lifetime and ordering discipline live in exactly one place. A per-stack
//! [`CacheAlignedSegmentLock`] covers every head observation and successor-link
//! dereference. This is required because a mutation tag rejects a stale CAS but
//! cannot stop a concurrent decay sweep from releasing the observed mapping
//! before the pointer is dereferenced.
//!
//! The tagged head and advisory count share one cache-line-packed
//! [`TaggedStackState`]. Stack mutation already holds the lifetime lock, so the
//! packed state preserves the synchronization contract while removing one
//! per-stack alignment block. The resulting layout is benchmark-gated because
//! lock-free `len` readers can still observe the count while stack mutation
//! updates the same line.

use super::cache_aligned::{CacheAlignedSegmentLock, TaggedHead, TaggedStackState};
use core::sync::atomic::Ordering;
use mnemosyne_core::types::Segment;

/// A reclamation-safe stack of `Segment`s linked through `next_free_segment`,
/// with an advisory length counter.
pub(crate) struct TaggedSegmentStack {
    /// Serializes head observation through successor access or detachment, so
    /// a detached mapping can be released after `take_all` returns.
    mutation_lock: CacheAlignedSegmentLock,
    /// Tagged head and advisory count packed into one cache line.
    state: TaggedStackState,
}

const _: () = assert!(
    core::mem::size_of::<TaggedSegmentStack>()
        == core::mem::size_of::<CacheAlignedSegmentLock>()
            + core::mem::size_of::<TaggedStackState>()
);

impl TaggedSegmentStack {
    /// Creates a new empty stack.
    pub(crate) const fn new() -> Self {
        Self {
            mutation_lock: CacheAlignedSegmentLock::new(),
            state: TaggedStackState::new(),
        }
    }

    /// Advisory number of segments currently on the stack (a `Relaxed` load;
    /// callers tolerate a small skew under concurrency).
    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.state.len()
    }

    /// Splices a pre-linked chain onto the stack and adds `len` to the count.
    ///
    /// The single publishing path behind every push form: a lone segment is
    /// just the `head == tail`, `len == 1` chain, so the tag and ordering
    /// discipline is written once.
    ///
    /// # Safety
    ///
    /// The caller must hold this stack's `mutation_lock`, and `head`/`tail`
    /// must satisfy [`Self::push_chain`]'s contract.
    #[inline]
    unsafe fn splice_locked(&self, head: *mut Segment, tail: *mut Segment, len: usize) {
        debug_assert!(!head.is_null() && !tail.is_null() && len >= 1);
        let mut current = self.state.head.load(Ordering::Relaxed);
        loop {
            let current_ptr = TaggedHead::ptr(current);
            // SAFETY: by contract the caller owns the whole chain exclusively
            // until the publishing CAS succeeds, so linking `tail` to the
            // observed stack head is unobservable to other threads until then.
            unsafe {
                (*tail)
                    .next_free_segment
                    .store(current_ptr, core::sync::atomic::Ordering::Relaxed);
            }
            let next = TaggedHead::tagged_successor(head, current);
            match self.state.head.compare_exchange_weak(
                current,
                next,
                Ordering::Release,
                // Relaxed failure ordering is sound because the failure value
                // is only re-linked into the exclusively-owned chain tail,
                // never dereferenced. `pop` needs Acquire for the opposite
                // reason.
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
        self.state.count.fetch_add(len, Ordering::Relaxed);
    }

    /// Pushes `segment` onto the stack and increments the count.
    ///
    /// Equivalent to `push_chain(segment, segment, 1)`.
    ///
    /// # Safety
    ///
    /// `segment` must be a valid, initialized, exclusively-owned `Segment`;
    /// ownership transfers to the stack.
    #[inline]
    pub(crate) unsafe fn push(&self, segment: *mut Segment) {
        // SAFETY: a lone segment is a one-node chain (head == tail, len == 1).
        unsafe { self.push_chain(segment, segment, 1) }
    }

    /// Pushes `segment` unless the lifetime lock is busy, reporting whether it
    /// was pushed. Equivalent to `try_push_chain(segment, segment, 1)`.
    ///
    /// # Safety
    ///
    /// As [`Self::push`]; ownership transfers to the stack only when this
    /// returns `true`.
    #[inline]
    pub(crate) unsafe fn try_push(&self, segment: *mut Segment) -> bool {
        // SAFETY: forwarded — a lone segment is a one-node chain.
        unsafe { self.try_push_chain(segment, segment, 1) }
    }

    /// Pushes a pre-linked chain of `len` segments in a single tagged CAS and
    /// adds `len` to the count.
    ///
    /// The chain becomes the top of the stack in its existing `head → tail`
    /// link order: after the splice, `pop` returns `head` first, then the
    /// chain's successors in order, then whatever was on the stack before
    /// (including nodes pushed concurrently during the CAS loop, which end up
    /// below `tail`). Cost is one CAS and one lock acquisition regardless of
    /// `len`, versus `len` of each for element-wise re-pushing.
    ///
    /// # Safety
    ///
    /// `head` and `tail` must be non-null, exclusively-owned `Segment`s linked
    /// through `next_free_segment` such that `tail` is reached from `head` in
    /// exactly `len - 1` hops (`len >= 1`); no other thread may reach any chain
    /// node. Ownership of every chain node transfers to the stack.
    #[inline]
    pub(crate) unsafe fn push_chain(&self, head: *mut Segment, tail: *mut Segment, len: usize) {
        let _guard = self.mutation_lock.lock();
        // SAFETY: forwarded contract, with the lock held by the guard above.
        unsafe { self.splice_locked(head, tail, len) };
    }

    /// Chain form of [`Self::try_push`].
    ///
    /// # Safety
    ///
    /// As [`Self::push_chain`]; ownership transfers to the stack only when this
    /// returns `true`.
    #[inline]
    pub(crate) unsafe fn try_push_chain(
        &self,
        head: *mut Segment,
        tail: *mut Segment,
        len: usize,
    ) -> bool {
        let Some(_guard) = self.mutation_lock.try_lock() else {
            return false;
        };
        // SAFETY: forwarded contract, with the lock held by the guard above.
        unsafe { self.splice_locked(head, tail, len) };
        true
    }

    /// Pops the head segment, returning null when empty, decrementing the count
    /// and clearing the popped segment's `next_free_segment`.
    ///
    /// The mutation lock keeps the observed head mapping alive through the
    /// successor dereference and removal. The tag remains a structural check
    /// against stale head state but is not treated as a reclamation mechanism.
    #[inline]
    pub(crate) fn pop(&self) -> *mut Segment {
        let _guard = self.mutation_lock.lock();
        if self.state.count.load(Ordering::Relaxed) == 0 {
            return core::ptr::null_mut();
        }
        let mut current = self.state.head.load(Ordering::Acquire);
        loop {
            let current_ptr = TaggedHead::ptr(current);
            if current_ptr.is_null() {
                return core::ptr::null_mut();
            }
            // SAFETY: `current_ptr` was published by `push` (which wrote
            // `next_free_segment` before its Release CAS). Every load that can
            // produce the `current` we dereference here is Acquire — the initial
            // head load AND the CAS failure ordering below — so each synchronizes
            // with the pushing thread's Release CAS before the link is read. A
            // concurrent push/pop changes the head tag, so our CAS fails and
            // retries rather than acting on a stale successor.
            let next_ptr = unsafe {
                (*current_ptr)
                    .next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed)
            };
            let next = TaggedHead::tagged_successor(next_ptr, current);
            match self.state.head.compare_exchange_weak(
                current,
                next,
                Ordering::Acquire,
                // Acquire (not Relaxed): the failure value `actual` is
                // dereferenced on the next iteration, so this load must also
                // synchronize with the publishing push's Release CAS. `push`
                // keeps a Relaxed failure ordering because its failure value is
                // only stored into an exclusively-owned segment, never
                // dereferenced.
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.state.count.fetch_sub(1, Ordering::Relaxed);
                    // SAFETY: the successful CAS removed `current_ptr` from the
                    // shared stack, so this thread now exclusively owns it.
                    unsafe {
                        (*current_ptr)
                            .next_free_segment
                            .store(core::ptr::null_mut(), core::sync::atomic::Ordering::Relaxed);
                    }
                    return current_ptr;
                }
                Err(actual) => current = actual,
            }
        }
    }

    /// Detaches the entire chain in one atomic swap, returning its head (or
    /// null) and the prior count, leaving the stack empty.
    #[inline]
    pub(crate) fn take_all(&self) -> (*mut Segment, usize) {
        let _guard = self.mutation_lock.lock();
        let head = TaggedHead::ptr(self.state.head.swap_null(Ordering::Acquire));
        let count = self.state.count.swap(0, Ordering::Relaxed);
        (head, count)
    }
}

#[cfg(test)]
mod tests;
