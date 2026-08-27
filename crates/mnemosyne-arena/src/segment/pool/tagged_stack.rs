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
    /// # Safety
    ///
    /// `segment` must be a valid, initialized, exclusively-owned `Segment`;
    /// ownership transfers to the stack.
    #[inline]
    pub(crate) unsafe fn push(&self, segment: *mut Segment) {
        let _guard = self.mutation_lock.lock();
        // SAFETY: the guard above holds the mutation lock, and a lone
        // exclusively-owned segment is the one-node chain.
        unsafe { self.splice_locked(segment, segment, 1) };
    }

    /// Pushes `segment` unless the lifetime lock is busy, reporting whether it
    /// was pushed.
    ///
    /// For callers that must not wait — see
    /// [`CacheAlignedSegmentLock::try_lock`]. On `false` the caller retains
    /// ownership of `segment` and must place it elsewhere.
    ///
    /// # Safety
    ///
    /// As [`Self::push`]; ownership transfers to the stack only when this
    /// returns `true`.
    #[inline]
    pub(crate) unsafe fn try_push(&self, segment: *mut Segment) -> bool {
        let Some(_guard) = self.mutation_lock.try_lock() else {
            return false;
        };
        // SAFETY: as `push`, with the lock held by the guard above.
        unsafe { self.splice_locked(segment, segment, 1) };
        true
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
mod tests {
    use super::*;

    /// Allocates a boxed `Segment` through the production initializer.
    ///
    /// Boxing a zeroed value and filling a few fields by struct literal was the
    /// previous form; it bypassed `Segment::initialize`, so the page array and key
    /// schedule stayed zeroed, and `..zeroed()` silently absorbed every field added
    /// later. Running the real initializer costs one loop over the page array in a
    /// test and yields a segment whose invariants actually hold.
    fn boxed(raw: usize) -> *mut Segment {
        // SAFETY: `Segment` is pointers, integers, bools and arrays of the same, so
        // an all-zero bit pattern is a valid starting value for the initializer to
        // overwrite.
        let segment: *mut Segment = Box::into_raw(Box::new(unsafe { core::mem::zeroed() }));
        // SAFETY: `segment` is the live, uniquely-owned Box allocation just created,
        // and `Segment` requires only that the target be valid for writes here — the
        // pool tests never depend on `SEGMENT_ALIGN` addressing.
        unsafe {
            Segment::initialize(segment, segment.cast::<u8>().map_addr(|_| raw), 0);
        }
        segment
    }

    #[test]
    fn push_pop_is_lifo_and_tracks_count() {
        let stack = TaggedSegmentStack::new();
        assert_eq!(stack.len(), 0);
        assert_eq!(stack.pop(), core::ptr::null_mut());

        let a = boxed(0x1000);
        let b = boxed(0x2000);
        let c = boxed(0x3000);
        unsafe {
            stack.push(a);
            stack.push(b);
            stack.push(c);
        }
        assert_eq!(stack.len(), 3);
        // LIFO order, count decrements, links cleared.
        for expected in [c, b, a] {
            let popped = stack.pop();
            assert_eq!(popped, expected);
            unsafe {
                assert_eq!(
                    (*popped)
                        .next_free_segment
                        .load(core::sync::atomic::Ordering::Relaxed),
                    core::ptr::null_mut()
                );
            }
        }
        assert_eq!(stack.len(), 0);
        assert_eq!(stack.pop(), core::ptr::null_mut());

        for p in [a, b, c] {
            unsafe {
                let _ = Box::from_raw(p);
            }
        }
    }

    #[test]
    fn push_chain_splices_in_order_and_interleaves_with_push_pop() {
        let stack = TaggedSegmentStack::new();
        let below = boxed(0x0500);
        unsafe { stack.push(below) };

        // Build a private chain a -> b -> c and splice it in one CAS.
        let a = boxed(0x1000);
        let b = boxed(0x2000);
        let c = boxed(0x3000);
        unsafe {
            (*a).next_free_segment
                .store(b, core::sync::atomic::Ordering::Relaxed);
            (*b).next_free_segment
                .store(c, core::sync::atomic::Ordering::Relaxed);
            stack.push_chain(a, c, 3);
        }
        assert_eq!(stack.len(), 4);
        // Link integrity: chain order preserved, tail linked to the prior head.
        unsafe {
            assert_eq!(
                (*a).next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed),
                b
            );
            assert_eq!(
                (*b).next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed),
                c
            );
            assert_eq!(
                (*c).next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed),
                below
            );
        }

        // Interleave a plain push: it lands above the spliced chain.
        let d = boxed(0x4000);
        unsafe { stack.push(d) };
        assert_eq!(stack.len(), 5);

        // Pop order: d, then the chain head -> tail, then the pre-existing node.
        for expected in [d, a, b, c, below] {
            let popped = stack.pop();
            assert_eq!(popped, expected);
            unsafe {
                assert_eq!(
                    (*popped)
                        .next_free_segment
                        .load(core::sync::atomic::Ordering::Relaxed),
                    core::ptr::null_mut()
                );
            }
        }
        assert_eq!(stack.len(), 0);
        assert_eq!(stack.pop(), core::ptr::null_mut());

        for p in [a, b, c, d, below] {
            unsafe {
                let _ = Box::from_raw(p);
            }
        }
    }

    #[test]
    fn take_all_detaches_chain_and_count() {
        let stack = TaggedSegmentStack::new();
        let nodes: Vec<*mut Segment> = (0..6).map(|i| boxed(0x1000 * (i + 1))).collect();
        for &n in &nodes {
            unsafe { stack.push(n) };
        }
        assert_eq!(stack.len(), nodes.len());

        let (mut head, count) = stack.take_all();
        assert_eq!(count, nodes.len());
        assert_eq!(stack.len(), 0);
        let mut seen = 0usize;
        while !head.is_null() {
            seen += 1;
            head = unsafe {
                (*head)
                    .next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed)
            };
        }
        assert_eq!(seen, nodes.len());

        for n in nodes {
            unsafe {
                let _ = Box::from_raw(n);
            }
        }
    }

    #[test]
    fn concurrent_push_pop_conserves_every_segment() {
        use std::collections::HashSet;
        use std::sync::{Arc, Barrier};
        use std::thread;

        const THREADS: usize = 4;
        const NODES: usize = 12;
        const ITERS: usize = 20_000;

        let stack = Arc::new(TaggedSegmentStack::new());
        let originals: Vec<*mut Segment> =
            (0..NODES).map(|i| boxed(0x1_0000 + i * 0x100)).collect();
        for &n in &originals {
            unsafe { stack.push(n) };
        }

        let barrier = Arc::new(Barrier::new(THREADS));
        let mut handles = Vec::new();
        for _ in 0..THREADS {
            let stack = Arc::clone(&stack);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                for _ in 0..ITERS {
                    let p = stack.pop();
                    if !p.is_null() {
                        unsafe { stack.push(p) };
                    }
                }
            }));
        }
        for h in handles {
            h.join().expect("worker panicked");
        }

        // Conservation invariant: every original segment is recovered exactly
        // once (no loss, no duplicate/cycle) after the contention.
        let mut drained: HashSet<*mut Segment> = HashSet::new();
        let mut p = stack.pop();
        while !p.is_null() {
            assert!(drained.insert(p), "segment {p:?} drained twice");
            p = stack.pop();
        }
        assert_eq!(
            drained.len(),
            NODES,
            "lost or leaked a segment under contention"
        );
        for n in &originals {
            assert!(drained.contains(n), "original {n:?} not recovered");
        }

        for n in originals {
            unsafe {
                let _ = Box::from_raw(n);
            }
        }
    }

    /// The bounded push exists so a destructor never waits on a peer's critical
    /// section. It must therefore return — not block — while the lock is held,
    /// leave the stack untouched, and leave the segment with its caller.
    #[test]
    fn try_push_declines_a_held_lock_without_waiting() {
        let stack = TaggedSegmentStack::new();
        let resident = boxed(0x1000);
        let offered = boxed(0x2000);
        unsafe { stack.push(resident) };

        let held = stack.mutation_lock.lock();
        // Reaching the assertion at all is half the property: an unbounded
        // acquisition would hang here, which the runner's budget reports as the
        // deadlock it is.
        assert!(
            !unsafe { stack.try_push(offered) },
            "a bounded push must decline a held lock rather than wait for it"
        );
        assert_eq!(stack.len(), 1, "a declined push must not touch the stack");
        drop(held);

        // Ownership stayed with the caller, so the segment is still placeable.
        assert!(unsafe { stack.try_push(offered) });
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.pop(), offered);
        assert_eq!(stack.pop(), resident);

        for p in [resident, offered] {
            unsafe {
                let _ = Box::from_raw(p);
            }
        }
    }

    /// The chain form carries the same decline-rather-than-wait contract, and
    /// on success splices exactly as the blocking `push_chain` does — both now
    /// route through one `splice_locked`.
    #[test]
    fn try_push_chain_declines_a_held_lock_then_splices_in_order() {
        let stack = TaggedSegmentStack::new();
        let below = boxed(0x0500);
        unsafe { stack.push(below) };

        let a = boxed(0x1000);
        let b = boxed(0x2000);
        let c = boxed(0x3000);
        unsafe {
            (*a).next_free_segment
                .store(b, core::sync::atomic::Ordering::Relaxed);
            (*b).next_free_segment
                .store(c, core::sync::atomic::Ordering::Relaxed);
        }

        let held = stack.mutation_lock.lock();
        assert!(
            !unsafe { stack.try_push_chain(a, c, 3) },
            "a bounded chain push must decline a held lock"
        );
        assert_eq!(stack.len(), 1, "a declined push must not touch the stack");
        // The private chain is intact, so the caller can retry it whole.
        unsafe {
            assert_eq!(
                (*a).next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed),
                b
            );
            assert_eq!(
                (*b).next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed),
                c
            );
        }
        drop(held);

        assert!(unsafe { stack.try_push_chain(a, c, 3) });
        assert_eq!(stack.len(), 4);
        for expected in [a, b, c, below] {
            let popped = stack.pop();
            assert_eq!(popped, expected);
            unsafe {
                assert_eq!(
                    (*popped)
                        .next_free_segment
                        .load(core::sync::atomic::Ordering::Relaxed),
                    core::ptr::null_mut()
                );
            }
        }
        assert_eq!(stack.len(), 0);

        for p in [a, b, c, below] {
            unsafe {
                let _ = Box::from_raw(p);
            }
        }
    }

    #[test]
    fn detach_waits_for_active_head_observer() {
        use core::sync::atomic::AtomicPtr;
        use std::sync::{Arc, Barrier, mpsc};
        use std::thread;

        let stack = Arc::new(TaggedSegmentStack::new());
        let bottom = boxed(0x1000);
        let top = boxed(0x2000);
        unsafe {
            stack.push(bottom);
            stack.push(top);
        }

        // Model a pop that has entered the head-observation critical section.
        // A concurrent decay detach must not return the chain until that
        // observer releases its guard; only then may its caller unmap nodes.
        let observer = stack.mutation_lock.lock();
        let rendezvous = Arc::new(Barrier::new(2));
        let detached_head = Arc::new(AtomicPtr::new(core::ptr::null_mut()));
        let (result_tx, result_rx) = mpsc::channel();
        let worker_stack = Arc::clone(&stack);
        let worker_rendezvous = Arc::clone(&rendezvous);
        let worker_head = Arc::clone(&detached_head);
        let worker = thread::spawn(move || {
            worker_rendezvous.wait();
            let (head, count) = worker_stack.take_all();
            worker_head.store(head, Ordering::Relaxed);
            result_tx.send(count).expect("result receiver remains live");
        });

        rendezvous.wait();
        assert_eq!(
            result_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty),
            "detach returned while a head observer still held the lifetime lock"
        );
        assert_eq!(stack.len(), 2);
        drop(observer);

        let count = result_rx.recv().expect("detach result is produced");
        worker.join().expect("detach worker did not panic");
        let head = detached_head.load(Ordering::Relaxed);
        assert_eq!(head, top);
        assert_eq!(count, 2);
        assert_eq!(stack.len(), 0);
        unsafe {
            assert_eq!(
                (*head)
                    .next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed),
                bottom
            );
            assert_eq!(
                (*bottom)
                    .next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed),
                core::ptr::null_mut()
            );
            let _ = Box::from_raw(top);
            let _ = Box::from_raw(bottom);
        }
    }
}
