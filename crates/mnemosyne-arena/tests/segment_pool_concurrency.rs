//! Conservation stress test for the reclamation-safe segment pool.
//!
//! `NodeSegmentPool` protects every head/link observation with its stack's
//! lifetime lock and retains a wrapping mutation tag in the packed head. This
//! test hammers concurrent pop/push and asserts
//! the core invariant: under arbitrary interleaving **no segment is ever lost or
//! duplicated**. It is the conservation check the pre-existing
//! `test_concurrent_aba_safeness` lacked (that test only drained whatever it
//! found, so a lost segment went undetected).

use mnemosyne_arena::GlobalSegmentPool;
use mnemosyne_core::types::Segment;
use std::collections::HashSet;
use std::sync::{Arc, Barrier};
use std::thread;

/// Heap-allocates a dummy `Segment` on NUMA node 0. The pool reads only
/// `numa_node` (for routing) and `next_free_segment` (the stack link).
fn boxed_segment(raw: usize) -> *mut Segment {
    // SAFETY: `Segment` is composed of pointers/integers/bools/arrays, so an
    // all-zero bit pattern is a valid empty instance.
    Box::into_raw(Box::new(Segment {
        raw_alloc_ptr: raw as *mut u8,
        next_free_segment: core::ptr::null_mut(),
        numa_node: 0,
        ..unsafe { core::mem::zeroed() }
    }))
}

#[test]
fn segment_pool_concurrent_push_pop_conserves_every_segment() {
    const THREADS: usize = 4;
    const SEGMENTS: usize = 12;
    const ITERS: usize = 20_000;

    let pool = Arc::new(GlobalSegmentPool::new());

    let mut originals: Vec<*mut Segment> = Vec::with_capacity(SEGMENTS);
    for i in 0..SEGMENTS {
        let seg = boxed_segment(0x10_000 + i * 0x1_000);
        originals.push(seg);
        // SAFETY: `seg` is a freshly-owned segment; ownership transfers to the
        // pool. `push_unbounded` applies no cap.
        unsafe {
            pool.push_unbounded(seg);
        }
    }

    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::with_capacity(THREADS);
    for _ in 0..THREADS {
        let pool = Arc::clone(&pool);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for _ in 0..ITERS {
                if let Some(seg) = pool.pop() {
                    // SAFETY: `seg` is exclusively owned until re-pushed within
                    // this same iteration; ownership transfers back to the pool.
                    unsafe {
                        pool.push_unbounded(seg);
                    }
                }
            }
        }));
    }
    for h in handles {
        h.join().expect("a worker thread panicked");
    }

    // Every worker re-pushes whatever it pops within the same iteration, so all
    // SEGMENTS are cached again. Drain and assert exact conservation.
    let mut drained: HashSet<*mut Segment> = HashSet::with_capacity(SEGMENTS);
    while let Some(seg) = pool.pop() {
        assert!(
            drained.insert(seg),
            "segment {seg:?} drained twice — duplication or a cycle in the stack"
        );
        // SAFETY: `seg` is exclusively owned after the pop; a popped node must
        // have a cleared link.
        unsafe {
            assert_eq!(
                (*seg).next_free_segment,
                core::ptr::null_mut(),
                "popped segment {seg:?} still has a dangling next link"
            );
        }
    }
    assert_eq!(
        drained.len(),
        SEGMENTS,
        "lost or leaked a segment under contention (recovered {})",
        drained.len()
    );
    for seg in &originals {
        assert!(
            drained.contains(seg),
            "original segment {seg:?} was not recovered"
        );
    }

    for seg in originals {
        // SAFETY: each original was created by `Box::into_raw`, drained exactly
        // once above, and is freed exactly once here.
        unsafe {
            let _ = Box::from_raw(seg);
        }
    }
}
