//! Tests for [`super::TaggedSegmentStack`]: LIFO order, chain splicing,
//! concurrent conservation, and lock-free `try_push` behaviour.

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
