//! Segment reclamation: what happens to an allocator's owned segments when it
//! tears down.
//!
//! MN-447 covered page recycling and recorded this half as unreachable, on the
//! reading that reclamation only runs from `ThreadAllocator::drop` and so
//! finishes after any assertion could observe it. That is not so —
//! `reclaim_owned_segments` is a method, and a test can call it and then look.
//!
//! The branch that matters is the one deciding a segment's sink. A segment with
//! no live allocations can go back to the pool for reuse; a segment still
//! holding live blocks cannot be unmapped at all, because the pointers its
//! former owner handed out are still in use, so the orphan pool is its only
//! destination. Picking the wrong branch is a use-after-free handed to whoever
//! still holds a block.

use super::super::*;
// The recording backend rather than the raw `DefaultBackend`:
// `backend_memory_stats` counts through this wrapper, so a segment released by
// the raw backend leaves no trace in the unmap counter these tests read.
use mnemosyne_backend::MemoryBackendWrapper as Backend;
use mnemosyne_core::policy::StandardPolicy;

/// Blocks are 64 bytes, stamped with a per-block byte.
const BLOCK: usize = 64;

/// Reads `p[..BLOCK]` and asserts every byte still equals `stamp`.
///
/// Takes the block index rather than a formatted label on purpose: building
/// one would allocate through the very allocator whose state these tests are
/// inspecting.
///
/// # Safety
///
/// `p` must be live and readable for `BLOCK` bytes.
#[track_caller]
unsafe fn assert_block_intact(p: *mut u8, stamp: u8, index: usize, stage: &str) {
    // SAFETY: forwarded from this function's contract.
    let actual = unsafe { core::slice::from_raw_parts(p, BLOCK) };
    let expected = [stamp; BLOCK];
    if actual == expected {
        return;
    }
    let off = actual
        .iter()
        .position(|&b| b != stamp)
        .expect("slices differ, so some byte differs");
    panic!(
        "{stage} block #{index}: byte {off} is {:#x}, expected {stamp:#x} — the \
         segment was released while its blocks were still live",
        actual[off]
    );
}

/// Allocates `count` stamped blocks from `owner`.
fn fill(owner: &mut ThreadAllocator<Backend>, count: usize) -> std::vec::Vec<*mut u8> {
    let mut blocks = std::vec::Vec::with_capacity(count);
    for i in 0..count {
        // SAFETY: `owner` is a live allocator and BLOCK is a small size class.
        let p = unsafe { owner.alloc::<StandardPolicy>(BLOCK) };
        assert!(!p.is_null(), "allocation {i} failed");
        // SAFETY: `p` is a fresh block valid for BLOCK bytes.
        unsafe { core::ptr::write_bytes(p, stamp_for(i), BLOCK) };
        blocks.push(p);
    }
    blocks
}

const fn stamp_for(i: usize) -> u8 {
    (i as u8).wrapping_mul(37).wrapping_add(0xD1)
}

/// Frees every block through the cross-thread queue and drains it.
///
/// `owner` is a locally constructed allocator, not this thread's TLS one, so
/// `thread_free` sees an owner-token mismatch and routes each block to the
/// page's atomic queue — the same path a genuinely foreign thread takes.
/// Reclamation is what drains that queue and updates the live count.
fn free_all_and_reclaim(owner: &mut ThreadAllocator<Backend>, blocks: &[*mut u8]) {
    for &p in blocks {
        // SAFETY: each pointer came from `owner` above and is freed once.
        unsafe { crate::thread_free::<StandardPolicy, Backend>(p) };
    }
    owner.reclaim_owned_segments();
}

#[test]
fn reclaim_orphans_a_segment_whose_blocks_are_still_live() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    // SAFETY: the test lock is held, so no other test touches the pools.
    unsafe { super::fixtures::drain_all_pools() };

    let mut owner = ThreadAllocator::<Backend>::new();
    let blocks = fill(&mut owner, 4);
    assert!(
        owner.stats().current_thread_owned_segments >= 1,
        "the owner must hold a segment before it can be reclaimed"
    );

    // Tear down while every block is still outstanding.
    owner.reclaim_owned_segments();
    assert_eq!(
        owner.stats().current_thread_owned_segments,
        0,
        "reclamation must release the allocator's claim on every segment"
    );

    // The segment cannot have been unmapped: the blocks are still reachable by
    // whoever holds them, and they must still read back what was written.
    for (i, &p) in blocks.iter().enumerate() {
        // SAFETY: the block is live — that is precisely the claim under test.
        unsafe { assert_block_intact(p, stamp_for(i), i, "orphaned") };
    }

    // Live segments have exactly one sink, so a fresh allocator must be able to
    // adopt this one rather than mapping a new segment from the OS.
    let mut adopter = ThreadAllocator::<Backend>::new();
    // SAFETY: `adopter` is live and BLOCK is a small size class.
    let fresh_block = unsafe { adopter.alloc::<StandardPolicy>(BLOCK) };
    assert!(!fresh_block.is_null(), "adopter allocation failed");
    assert!(
        adopter.stats().orphan_segments_adopted >= 1,
        "the reclaimed segment did not reach the orphan pool: the adopter \
         mapped a fresh segment instead of taking it"
    );

    // The originals must have survived the adoption too.
    for (i, &p) in blocks.iter().enumerate() {
        // SAFETY: still live; adoption transfers ownership, not the payload.
        unsafe { assert_block_intact(p, stamp_for(i), i, "adopted") };
    }

    let mut all = blocks;
    all.push(fresh_block);
    free_all_and_reclaim(&mut adopter, &all);
}

#[test]
fn reclaim_returns_an_emptied_segment_to_the_pool() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    // SAFETY: the test lock is held, so no other test touches the pools.
    unsafe { super::fixtures::drain_all_pools() };

    // Two disposals are correct here, and which one happens is a tuning
    // decision rather than a property: the pool caches the segment for reuse,
    // or the backend hands its mapping back to the OS. `MAX_RETAINED_SEGMENTS`
    // defaults to zero under `cfg(miri)` so that intentionally cached mappings
    // are not reported as leaks, so asserting the cache grew would pass
    // natively and fail under Miri while the allocator did nothing wrong.
    let retained_before = mnemosyne_arena::arena_memory_stats::<Backend>().retained_free_segments;
    let unmapped_before = mnemosyne_backend::backend_memory_stats().unmap_calls;

    let mut owner = ThreadAllocator::<Backend>::new();
    let blocks = fill(&mut owner, 4);
    assert!(owner.stats().current_thread_owned_segments >= 1);

    // Free everything first, so reclamation finds no live allocation and takes
    // the other branch.
    free_all_and_reclaim(&mut owner, &blocks);

    assert_eq!(
        owner.stats().current_thread_owned_segments,
        0,
        "reclamation must release the allocator's claim on every segment"
    );

    let retained_after = mnemosyne_arena::arena_memory_stats::<Backend>().retained_free_segments;
    let unmapped_after = mnemosyne_backend::backend_memory_stats().unmap_calls;
    assert!(
        retained_after > retained_before || unmapped_after > unmapped_before,
        "an emptied segment was neither pooled nor released: retained went \
         {retained_before} -> {retained_after}, unmap calls went \
         {unmapped_before} -> {unmapped_after}"
    );

    // Nothing should be sitting in the orphan pool: orphaning a segment with no
    // live blocks would strand a mapping no one is going to adopt.
    use mnemosyne_arena::HasSegmentPool;
    // The test lock is held, so no allocator can race this pop.
    let stranded = <Backend as HasSegmentPool>::global_orphan_pool().pop();
    assert!(
        stranded.is_none(),
        "an emptied segment was orphaned rather than pooled"
    );
}
