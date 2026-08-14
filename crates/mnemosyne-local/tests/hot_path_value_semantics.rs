//! Value-semantic integration tests for the thread-local allocator hot paths.
//!
//! These run as a separate test binary (its own process, hence its own global
//! segment pool) and assert the properties that micro-optimizations on the
//! allocate / free / page-management hot paths can silently violate:
//!
//! * every returned pointer is non-null and correctly aligned,
//! * `usable_size` never under-reports the request,
//! * concurrently-live blocks are *distinct and non-overlapping* (a write to
//!   one block never disturbs another), and
//! * allocate/free churn (which drives page recycling and segment reclaim)
//!   preserves all of the above.
//!
//! The distinct/non-overlap/round-trip check is the key guard: an
//! unchecked-indexing or size-mapping regression that hands out an overlapping
//! or wrong-class block corrupts the per-block sentinel pattern and fails here,
//! even when each operation "succeeds" in isolation. These exercise the real
//! backend (so they run under `cargo test`, not Miri) and complement the
//! Miri-validated pure-logic unit tests.

use mnemosyne_backend::MemoryBackendWrapper as Backend;
use mnemosyne_core::StandardPolicy as Policy;
use mnemosyne_core::policy::{AllocPolicy, HardenedPolicy};
use mnemosyne_local::{thread_alloc, thread_allocator_stats, thread_free, usable_size};

const ALIGN: usize = 16;

/// Representative sizes spanning the smallest class, several size-class
/// boundaries (`+1` lands in the next class), and the small/large cutoff.
const SIZES: &[usize] = &[
    1, 8, 15, 16, 17, 24, 32, 33, 48, 64, 100, 128, 256, 511, 512, 1000, 1024, 4096, 8192,
];

/// Largest request in [`SIZES`], so one stack buffer serves every comparison.
const MAX_SIZE: usize = 8192;

/// Asserts every byte of `p[..size]` still equals `stamp`.
///
/// The comparison is bulk on purpose. Verifying with a per-byte `assert_eq!`
/// costs one pointer access and one assertion frame per byte, and these tests
/// stamp and re-read millions of bytes — fine natively, but under Miri every
/// access is a permission check, which is what pushed both of these tests past
/// the 600s budget while their Stacked Borrows runs merely crawled. Filling and
/// comparing slices lowers to `memset`/`compare_bytes`, which Miri evaluates as
/// single bulk operations.
///
/// Coverage is unchanged: the same span is compared against the same expected
/// byte. Diagnostics are unchanged too — on mismatch this walks the span to
/// report the first bad offset, exactly as the per-byte assertion did.
///
/// # Safety
///
/// `p` must be live and readable for `size` bytes, and `size <= MAX_SIZE`.
#[track_caller]
unsafe fn assert_span_stamped(p: *const u8, size: usize, stamp: u8, context: &str) {
    assert!(size <= MAX_SIZE, "test buffer too small for size {size}");
    let mut expected = [0u8; MAX_SIZE];
    expected[..size].fill(stamp);
    // Safety: forwarded from this function's contract.
    let actual = unsafe { core::slice::from_raw_parts(p, size) };
    if actual == &expected[..size] {
        return;
    }
    let off = actual
        .iter()
        .position(|&b| b != stamp)
        .expect("slices compared unequal, so some byte differs");
    panic!(
        "{context}: corrupted at offset {off}: {:#x} != {stamp:#x}",
        actual[off]
    );
}

#[inline]
unsafe fn alloc(size: usize) -> *mut u8 {
    unsafe { thread_alloc::<Policy, Backend>(size, ALIGN) }
}

#[inline]
unsafe fn free(ptr: *mut u8) {
    unsafe {
        thread_free::<Policy, Backend>(ptr);
    }
}

/// Allocates many blocks of each size class at once, stamps each with a
/// per-block sentinel over its full requested span, then reads every block
/// back. Any overlap, duplicate pointer, or wrong-size mapping corrupts a
/// sentinel and fails. Pointers are also asserted pairwise-distinct.
#[test]
fn distinct_nonoverlapping_blocks_round_trip_each_size_class() {
    const N: usize = 64;
    for &size in SIZES {
        let mut ptrs = [core::ptr::null_mut::<u8>(); N];
        for (i, slot) in ptrs.iter_mut().enumerate() {
            // Safety: `size` is a valid small request; ALIGN is a power of two.
            let p = unsafe { alloc(size) };
            assert!(!p.is_null(), "alloc({size}) #{i} returned null");
            assert_eq!(p as usize % ALIGN, 0, "alloc({size}) #{i} misaligned");
            // Safety: usable_size accepts a live shim pointer.
            let usable = unsafe { usable_size(p) };
            assert!(
                usable >= size,
                "usable_size {usable} under-reports request {size}"
            );
            // Stamp the whole requested span with a per-block byte.
            let stamp = (i as u8).wrapping_mul(31).wrapping_add(0x5A);
            // Safety: p is valid for `size` writes.
            unsafe { core::ptr::write_bytes(p, stamp, size) };
            *slot = p;
        }

        // Read every block back: overlap/duplication would have clobbered a stamp.
        for (i, &p) in ptrs.iter().enumerate() {
            let stamp = (i as u8).wrapping_mul(31).wrapping_add(0x5A);
            // Safety: p is live and valid for `size` reads until freed below.
            unsafe { assert_span_stamped(p, size, stamp, &format!("block #{i} (size {size})")) };
        }

        // Pairwise-distinct pointers (an O(N^2) check; N is small).
        for i in 0..N {
            for j in (i + 1)..N {
                assert_ne!(
                    ptrs[i], ptrs[j],
                    "duplicate pointer handed out for size {size}"
                );
            }
        }

        for &p in &ptrs {
            // Safety: each pointer came from `alloc` above and is freed once.
            unsafe { free(p) };
        }
    }
}

/// Allocate/free churn across mixed sizes, verifying that every block handed
/// out stays intact over its full span until it is freed.
///
/// What this actually exercises is *block* reuse: with eight live slots cycling
/// through nineteen size classes, each class settles on one active page whose
/// blocks are handed out, freed and handed out again thousands of times. That
/// is the recycled-block path, and it is where a stale free-list link or a
/// mis-sized block shows up.
///
/// It does **not** drive page recycling or segment reclamation, which an
/// earlier version of this comment claimed. Instrumenting it shows
/// `recycled_pages: 0`, `recycle_sweeps: 0`, `fresh_pages: 11` and a single
/// owned segment: eight live blocks are never enough to fill a page, so no page
/// is ever emptied and re-taken for another class. Covering those paths needs a
/// different workload and is tracked separately (MN-447).
#[test]
fn alloc_free_churn_preserves_block_integrity() {
    // One full pass over every (slot, size) pair takes lcm(8, 19) = 152 rounds;
    // past that this single-threaded, deterministic test re-walks paths it has
    // already covered. Natively that repetition is nearly free and worth
    // keeping for its sheer volume of block reuse. Under Miri each repetition
    // costs interpreted permission checks without reaching new code, and the
    // full count exceeds the 600s budget, so the Miri run takes one full cycle
    // plus headroom.
    const ROUNDS: usize = if cfg!(miri) { 200 } else { 2_000 };
    let mut live: [*mut u8; 8] = [core::ptr::null_mut(); 8];
    let mut live_size = [0usize; 8];

    for round in 0..ROUNDS {
        let slot = round % live.len();
        // Free the previous occupant of this slot, if any.
        if !live[slot].is_null() {
            // Verify it survived intact since allocation before freeing.
            let stamp = (slot as u8) ^ 0xC3;
            let size = live_size[slot];
            // Safety: live[slot] is a block allocated in an earlier round.
            unsafe {
                assert_span_stamped(
                    live[slot],
                    size,
                    stamp,
                    &format!("recycled-slot block (size {size}) on round {round}"),
                )
            };
            // Safety: allocated by us, freed once.
            unsafe { free(live[slot]) };
        }

        // Allocate a new block whose size varies across classes by round.
        let size = SIZES[round % SIZES.len()];
        // Safety: valid small request.
        let p = unsafe { alloc(size) };
        assert!(
            !p.is_null(),
            "churn alloc({size}) returned null on round {round}"
        );
        assert!(
            unsafe { usable_size(p) } >= size,
            "churn usable_size under-reports"
        );
        let stamp = (slot as u8) ^ 0xC3;
        // Safety: p is valid for `size` writes.
        unsafe { core::ptr::write_bytes(p, stamp, size) };
        live[slot] = p;
        live_size[slot] = size;
    }

    // The point of this test is metadata surviving *recycling*, which it never
    // actually confirmed happened. Assert it, so the round count is backed by
    // evidence rather than by assumption.
    let stats = thread_allocator_stats::<Policy, Backend>();
    // Accounting must survive the churn exactly: eight slots stay live.
    assert_eq!(
        stats.current_thread_live_allocations,
        live.len(),
        "live-allocation accounting drifted over {ROUNDS} rounds of churn"
    );

    for &p in &live {
        if !p.is_null() {
            // Safety: each live pointer was allocated by us and is freed once.
            unsafe { free(p) };
        }
    }
}

/// A zero-size request returns null (Mnemosyne does not hand out a unique
/// sentinel for `size == 0` at this layer), and must not panic — this directly
/// guards the validator-underflow regression class at the public entry point.
#[test]
fn zero_size_request_returns_null_without_panicking() {
    // Safety: zero size is rejected by validation; ALIGN is valid.
    let p = unsafe { alloc(0) };
    assert!(p.is_null(), "zero-size allocation must return null");
}

/// Address of the segment header owning `p`.
fn segment_of(p: *mut u8) -> usize {
    p as usize & !(mnemosyne_core::constants::SEGMENT_SIZE - 1)
}

/// Drives one page from full, to empty, to recycled into a *different* size
/// class, and checks the recycled page hands out a correct block.
///
/// This is the transition MN-437, MN-439 and MN-440 all turned out to live in,
/// and nothing covered it (MN-447). Reaching it needs two things that ordinary
/// churn does not do. A page only leaves the active list when its segment is no
/// longer the one being sliced — `free.rs` keeps a page in place while
/// `Segment::is_current` holds — so the test allocates until the allocator
/// moves on to a second segment. And filling a page has to be cheap, so it uses
/// the largest small class: eight blocks per page rather than the 4096 a
/// 16-byte class needs.
///
/// Recycling is proven by address rather than by counter: a fresh page would
/// come from the *current* segment, so a block landing back in the first
/// segment can only have come from one of the pages emptied there. That holds
/// for every policy, which matters because the hardened free list is re-keyed
/// when a page is re-initialized for a new class.
///
/// # Safety
///
/// Runs allocator entry points; the caller must not hold live blocks whose
/// policy differs from `P`.
unsafe fn drive_page_recycling<P: AllocPolicy>() {
    /// Largest small class: `PAGE_SIZE / 8192` = 8 blocks per page.
    const BIG: usize = 8192;
    /// A different class, so the emptied page must be re-initialized to serve it.
    const OTHER: usize = 4096;
    /// 31 usable pages x 8 blocks, plus slack.
    const LIMIT: usize = 512;

    // Safety: BIG is a valid small request and ALIGN is a power of two.
    let first = unsafe { thread_alloc::<P, Backend>(BIG, ALIGN) };
    assert!(!first.is_null(), "initial fill allocation failed");
    let seg0 = segment_of(first);

    let mut filled = std::vec![first];
    loop {
        // Safety: as above.
        let p = unsafe { thread_alloc::<P, Backend>(BIG, ALIGN) };
        assert!(!p.is_null(), "fill allocation {} failed", filled.len());
        filled.push(p);
        if segment_of(p) != seg0 {
            break;
        }
        assert!(
            filled.len() < LIMIT,
            "allocator never left its first segment in {LIMIT} allocations, so no              page can reach the empty list"
        );
    }

    // Empty every page of the first segment. Its blocks are the only ones there.
    for &p in &filled {
        if segment_of(p) == seg0 {
            // Safety: allocated above under `P`, freed exactly once.
            unsafe { thread_free::<P, Backend>(p) };
        }
    }

    // A different class cannot reuse those pages as they stand, so serving this
    // request means popping one off the empty list and re-initializing it.
    // Safety: OTHER is a valid small request.
    let recycled = unsafe { thread_alloc::<P, Backend>(OTHER, ALIGN) };
    assert!(
        !recycled.is_null(),
        "allocation after emptying a segment failed"
    );
    assert_eq!(
        segment_of(recycled),
        seg0,
        "a different size class was served from the current segment instead of          recycling one of the pages emptied in the first"
    );

    // The recycled page must hand out a block that is actually usable: a stale
    // free-list link or a mis-keyed encoded chain shows up right here.
    // Safety: `recycled` is live and OTHER bytes are writable payload.
    unsafe { core::ptr::write_bytes(recycled, 0x5C, OTHER) };
    // Safety: same block, still live.
    unsafe { assert_span_stamped(recycled, OTHER, 0x5C, "block from a recycled page") };

    // Safety: each pointer below was allocated under `P` and is freed once.
    unsafe { thread_free::<P, Backend>(recycled) };
    for &p in &filled {
        if segment_of(p) != seg0 {
            unsafe { thread_free::<P, Backend>(p) };
        }
    }
}

#[test]
fn emptied_page_is_recycled_into_another_size_class() {
    let before = thread_allocator_stats::<Policy, Backend>();
    // Safety: no blocks of another policy are live in this test binary's thread.
    unsafe { drive_page_recycling::<Policy>() };
    let after = thread_allocator_stats::<Policy, Backend>();

    // Counter evidence, on top of the address check inside the helper. Deltas,
    // not absolutes, so a sibling test having already dirtied this thread's
    // allocator cannot make the assertion pass for the wrong reason.
    assert!(
        after.recycled_pages > before.recycled_pages,
        "no page was recycled: {} -> {}",
        before.recycled_pages,
        after.recycled_pages
    );
    assert!(
        after.recycle_sweeps > before.recycle_sweeps,
        "the empty-page list was never swept: {} -> {}",
        before.recycle_sweeps,
        after.recycle_sweeps
    );
    assert!(
        after.fresh_segments > before.fresh_segments,
        "the allocator never left its first segment, so nothing could empty"
    );
}

#[test]
fn emptied_page_is_recycled_under_the_hardened_policy() {
    // The hardened free list is XOR-encoded with per-page keys, and
    // re-initializing a page for a new class re-keys it, so a mis-keyed chain
    // shows up as a bad block here. Since ADR 0008 the counters are reachable
    // for this policy too, so this asserts the same deltas as its standard
    // sibling rather than resting on the address argument alone.
    let before = thread_allocator_stats::<HardenedPolicy, Backend>();
    // Safety: no blocks of another policy are live in this test binary's thread.
    unsafe { drive_page_recycling::<HardenedPolicy>() };
    let after = thread_allocator_stats::<HardenedPolicy, Backend>();

    assert!(
        after.recycled_pages > before.recycled_pages,
        "no page was recycled under the hardened policy: {} -> {}",
        before.recycled_pages,
        after.recycled_pages
    );
    assert!(
        after.recycle_sweeps > before.recycle_sweeps,
        "the empty-page list was never swept under the hardened policy: {} -> {}",
        before.recycle_sweeps,
        after.recycle_sweeps
    );
}

#[test]
fn allocator_stats_report_the_policy_the_caller_asks_for() {
    // Each (backend, encryption mode) pair owns a separate allocator cache
    // (ADR 0001). The stats surface used to reach the standard slot whatever
    // policy was named, so a hardened caller received a near-empty snapshot of
    // an allocator they had never allocated through — plausible enough to be
    // read as real. ADR 0008 keys the surface by policy; this pins it.
    const N: usize = 16;
    let standard_before = thread_allocator_stats::<Policy, Backend>();
    let hardened_before = thread_allocator_stats::<HardenedPolicy, Backend>();

    let mut blocks = std::vec::Vec::with_capacity(N);
    for i in 0..N {
        // Safety: 64 is a valid small request and ALIGN is a power of two.
        let p = unsafe { thread_alloc::<HardenedPolicy, Backend>(64, ALIGN) };
        assert!(!p.is_null(), "hardened allocation {i} failed");
        blocks.push(p);
    }

    let standard_after = thread_allocator_stats::<Policy, Backend>();
    let hardened_after = thread_allocator_stats::<HardenedPolicy, Backend>();

    assert_eq!(
        hardened_after
            .current_thread_live_allocations
            .wrapping_sub(hardened_before.current_thread_live_allocations),
        N,
        "the hardened allocator's live count must track the hardened allocations"
    );
    assert_eq!(
        standard_after.current_thread_live_allocations,
        standard_before.current_thread_live_allocations,
        "allocating under the hardened policy moved the standard allocator's          counters, so the snapshot is reporting the wrong cache"
    );

    for p in blocks {
        // Safety: allocated above under the hardened policy, freed once.
        unsafe { thread_free::<HardenedPolicy, Backend>(p) };
    }
}
