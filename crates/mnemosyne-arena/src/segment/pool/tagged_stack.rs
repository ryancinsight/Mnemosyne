//! ABA-immune lock-free intrusive Treiber stack of [`Segment`]s with an
//! advisory retained count — the single authoritative implementation of the
//! tagged-pointer CAS loop shared by the huge-allocation cache
//! ([`super::huge_pool`]) and the segment pool ([`super::list`]).
//!
//! Both pools previously hand-drove the identical push / pop / `take_all` CAS
//! loops over [`CacheAlignedAtomicPtr`]; centralizing them here means the
//! ordering discipline and the ABA-tag invariant (a stale single-element-pop
//! CAS fails on the high-bit mutation tag even when the head address has cycled
//! back) live in exactly one place. The struct holds only atomics, so `Send` /
//! `Sync` are compiler-derived rather than hand-asserted.
//!
//! The head and count keep their own cache lines ([`CacheAlignedAtomicPtr`] /
//! [`CacheAlignedAtomicUsize`] are each `#[repr(align(64))]`), matching the
//! prior per-atomic-isolation layout; consolidating into a single line is a
//! separate, benchmark-gated experiment (see backlog).

use super::cache_aligned::{CacheAlignedAtomicPtr, CacheAlignedAtomicUsize};
use core::sync::atomic::Ordering;
use mnemosyne_core::types::Segment;

/// A lock-free, ABA-immune Treiber stack of `Segment`s linked through
/// `next_free_segment`, with an advisory length counter.
pub(crate) struct TaggedSegmentStack {
    /// Tagged head: low 48 bits are the head segment address, high bits a
    /// wrapping mutation tag that defeats ABA on single-element `pop`.
    head: CacheAlignedAtomicPtr,
    /// Advisory count of segments currently on the stack.
    count: CacheAlignedAtomicUsize,
}

impl TaggedSegmentStack {
    /// Creates a new empty stack.
    pub(crate) const fn new() -> Self {
        Self {
            head: CacheAlignedAtomicPtr::new(core::ptr::null_mut()),
            count: CacheAlignedAtomicUsize::new(0),
        }
    }

    /// Advisory number of segments currently on the stack (a `Relaxed` load;
    /// callers tolerate a small skew under concurrency).
    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.count.value.load(Ordering::Relaxed)
    }

    /// Pushes `segment` onto the stack and increments the count.
    ///
    /// # Safety
    ///
    /// `segment` must be a valid, initialized, exclusively-owned `Segment`;
    /// ownership transfers to the stack.
    #[inline]
    pub(crate) unsafe fn push(&self, segment: *mut Segment) {
        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            let current_ptr = CacheAlignedAtomicPtr::ptr(current);
            // SAFETY: by contract the caller owns `segment` exclusively until the
            // publishing CAS succeeds, so writing its link first is unobservable
            // to other threads until then.
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

    /// Pops the head segment, returning null when empty, decrementing the count
    /// and clearing the popped segment's `next_free_segment`.
    ///
    /// ABA-immune: the head tag increments on every push/pop, so a stale CAS
    /// fails even when the head address has cycled back to the same value.
    #[inline]
    pub(crate) fn pop(&self) -> *mut Segment {
        if self.len() == 0 {
            return core::ptr::null_mut();
        }
        let mut current = self.head.load(Ordering::Acquire);
        loop {
            let current_ptr = CacheAlignedAtomicPtr::ptr(current);
            if current_ptr.is_null() {
                return core::ptr::null_mut();
            }
            // SAFETY: `current_ptr` was published by `push` (which wrote
            // `next_free_segment` before its Release CAS); the Acquire load/CAS
            // observes that link. A concurrent push/pop changes the head tag, so
            // our CAS fails and retries rather than acting on a stale successor.
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
                    // SAFETY: the successful CAS removed `current_ptr` from the
                    // shared stack, so this thread now exclusively owns it.
                    unsafe {
                        (*current_ptr).next_free_segment = core::ptr::null_mut();
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
        let head = CacheAlignedAtomicPtr::ptr(self.head.swap_null(Ordering::Acquire));
        let count = self.count.value.swap(0, Ordering::Relaxed);
        (head, count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(seed: usize) -> *mut Segment {
        Box::into_raw(Box::new(Segment {
            raw_alloc_ptr: seed as *mut u8,
            next_free_segment: core::ptr::null_mut(),
            ..unsafe { core::mem::zeroed() }
        }))
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
                assert_eq!((*popped).next_free_segment, core::ptr::null_mut());
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
            head = unsafe { (*head).next_free_segment };
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
        let originals: Vec<*mut Segment> = (0..NODES).map(|i| boxed(0x1_0000 + i * 0x100)).collect();
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

        // ABA-immunity invariant: every original segment is recovered exactly
        // once (no loss, no duplicate/cycle) after the contention.
        let mut drained: HashSet<*mut Segment> = HashSet::new();
        let mut p = stack.pop();
        while !p.is_null() {
            assert!(drained.insert(p), "segment {p:?} drained twice");
            p = stack.pop();
        }
        assert_eq!(drained.len(), NODES, "lost or leaked a segment under contention");
        for n in &originals {
            assert!(drained.contains(n), "original {n:?} not recovered");
        }

        for n in originals {
            unsafe {
                let _ = Box::from_raw(n);
            }
        }
    }
}
