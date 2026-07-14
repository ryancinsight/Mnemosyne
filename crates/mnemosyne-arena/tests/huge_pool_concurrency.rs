//! Concurrency stress test for the reclamation-safe huge-allocation cache.
//!
//! Its subtlest path is the exact-bucket first-fit, which pops
//! heads until one fits while stashing undersized segments in a private chain
//! and restoring them. This test hammers that path from
//! multiple threads and asserts the pool's core invariant: under arbitrary
//! interleaving, no cached segment is ever lost or duplicated and every popped
//! block satisfies its size request.
//!
//! The repo has no `loom` harness; consistent with the existing
//! `concurrent_push_pop_conserves_every_segment`, this is a std-thread + `Barrier` stress test
//! that exercises real interleavings on the public `GlobalHugePool` API.

use mnemosyne_arena::GlobalHugePool;
use mnemosyne_core::types::Segment;
use std::collections::HashSet;
use std::sync::{Arc, Barrier};
use std::thread;

/// Heap-allocates a dummy huge `Segment` carrying `block_size` on page 0. The
/// pool only reads `pages[0].block_size`, `next_free_segment`, and
/// `raw_alloc_ptr`, so a zeroed segment with those fields set is a faithful
/// stand-in without touching the OS backend.
fn boxed_huge_segment(raw: usize, block_size: usize) -> *mut Segment {
    // SAFETY: `Segment` is composed entirely of pointers, integers, bools, and
    // arrays thereof, so an all-zero bit pattern is a valid empty instance.
    let segment = Box::into_raw(Box::new(Segment {
        raw_alloc_ptr: raw as *mut u8,
        next_free_segment: core::ptr::null_mut(),
        ..unsafe { core::mem::zeroed() }
    }));
    // SAFETY: `segment` was just allocated and is exclusively owned here.
    unsafe {
        (*segment).pages[0].block_size = block_size;
    }
    segment
}

#[test]
fn huge_pool_concurrent_push_pop_conserves_every_segment() {
    const THREADS: usize = 8;
    const SEGMENTS: usize = 12;
    const ITERS: usize = 50_000;
    // Both sizes fall in the same huge bucket - bucket 1 covers `(16 KiB,
    // 32 KiB]` - so a 32 KiB request must scan past the 20 KiB heads and the
    // first-fit restash path runs concurrently across threads.
    const SMALL: usize = 20 * 1024;
    const LARGE: usize = 32 * 1024;
    const NODE: usize = 0;

    let pool = Arc::new(GlobalHugePool::new());

    let mut originals: Vec<*mut Segment> = Vec::with_capacity(SEGMENTS);
    for i in 0..SEGMENTS {
        let size = if i % 2 == 0 { SMALL } else { LARGE };
        let seg = boxed_huge_segment(0x40_000 + i * 0x1_000, size);
        originals.push(seg);
        // SAFETY: `seg` is a freshly-owned huge segment; ownership transfers to
        // the pool. The bucket cap (>= 1024) far exceeds SEGMENTS, so the push
        // cannot be rejected.
        unsafe {
            assert!(
                pool.try_push(seg, NODE),
                "initial try_push must be accepted"
            );
        }
    }

    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::with_capacity(THREADS);
    for t in 0..THREADS {
        let pool = Arc::clone(&pool);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            // Even threads chase the large class (forcing restash past small
            // heads); odd threads chase the small class (head usually fits).
            let request = if t % 2 == 0 { LARGE } else { SMALL };
            barrier.wait();
            for _ in 0..ITERS {
                // SAFETY: a popped segment is exclusively owned by this thread
                // until it is pushed back within the same iteration.
                if let Some(seg) = unsafe { pool.pop(request, NODE) } {
                    // SAFETY: `seg` is the just-popped, exclusively-owned block.
                    let block_size = unsafe { (*seg).pages[0].block_size };
                    assert!(
                        block_size >= request,
                        "pop({request}) returned an undersized block ({block_size})"
                    );
                    // SAFETY: ownership transfers back to the pool; cap is never
                    // reached (at most SEGMENTS blocks ever exist).
                    unsafe {
                        assert!(
                            pool.try_push(seg, NODE),
                            "re-push must be accepted under cap"
                        );
                    }
                }
            }
        }));
    }
    for h in handles {
        h.join().expect("a worker thread panicked");
    }

    // Every worker finishes an iteration having pushed back whatever it popped,
    // so all SEGMENTS are cached again. Drain and assert exact conservation: a
    // SMALL request fits every cached block (both size classes live in bucket
    // 1) and stays within the pop fit cap, which rejects requests more than
    // `HUGE_POP_FIT_CAP x` smaller than a bucket's blocks (so a size-1 drain
    // request would now correctly miss).
    let mut drained: HashSet<*mut Segment> = HashSet::with_capacity(SEGMENTS);
    // SAFETY: draining pops exclusively-owned segments; no other thread runs now.
    while let Some(seg) = unsafe { pool.pop(SMALL, NODE) } {
        assert!(
            drained.insert(seg),
            "segment {seg:?} drained twice - duplication or a cycle in the stack"
        );
        // SAFETY: `seg` is exclusively owned after the pop; a popped block must
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
        "lost or leaked a cached segment under contention (recovered {})",
        drained.len()
    );
    for seg in &originals {
        assert!(
            drained.contains(seg),
            "original segment {seg:?} was not recovered"
        );
    }

    for seg in originals {
        // SAFETY: each original was created by `Box::into_raw`, drained back out
        // exactly once above, and is freed exactly once here.
        unsafe {
            let _ = Box::from_raw(seg);
        }
    }
}
