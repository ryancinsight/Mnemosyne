//! Huge-pool tests: log2 bucketing, exact-bucket head restoration,
//! admission by actual bucket bytes, over-provisioned-bucket skipping, and
//! the runtime arena-stat counters.

use super::pool::{GlobalHugePool, HasSegmentPool};
use super::stats::arena_memory_stats;
use super::tests::FailingReleaseBackend;
use mnemosyne_core::types::Segment;
use std::boxed::Box;

#[test]
fn test_huge_pool_log2_bucketing() {
    use super::pool::huge_pool::huge_bucket_index;

    // Boundary at 16 KiB
    assert_eq!(huge_bucket_index(0), 0);
    assert_eq!(huge_bucket_index(16384), 0);
    assert_eq!(huge_bucket_index(16385), 1);

    // Power of two transitions
    assert_eq!(huge_bucket_index(32768), 1); // 32 KiB
    assert_eq!(huge_bucket_index(32769), 2);
    assert_eq!(huge_bucket_index(65536), 2); // 64 KiB
    assert_eq!(huge_bucket_index(65537), 3);
    assert_eq!(huge_bucket_index(1048576), 6); // 1 MiB
    assert_eq!(huge_bucket_index(1048577), 7);
    assert_eq!(huge_bucket_index(16 * 1024 * 1024), 10); // 16 MiB
    // Sizes beyond MAX_CACHED_HUGE_SIZE saturate to the last live bucket
    // (they are never pushed; only over-sized pop requests reach here).
    assert_eq!(huge_bucket_index(16 * 1024 * 1024 + 1), 10);
    assert_eq!(huge_bucket_index(512 * 1024 * 1024), 10);
}

#[test]
fn test_huge_pool_bucket_count_derived_from_max_cached_size() {
    use super::pool::huge_pool::{HUGE_SIZE_BUCKETS, huge_bucket_index};

    // SSOT pin: the bucket fan-out is exactly index(MAX_CACHED_HUGE_SIZE) + 1,
    // so no bucket is unreachable dead state under `try_push`'s size gate.
    assert_eq!(
        HUGE_SIZE_BUCKETS,
        huge_bucket_index(GlobalHugePool::MAX_CACHED_HUGE_SIZE) + 1
    );
    // 16 KiB (bucket 0) through 16 MiB (bucket 10) in log2 steps.
    assert_eq!(HUGE_SIZE_BUCKETS, 11);
}

/// Boxes a minimal `Segment` carrying only the page-0 `block_size` metadata
/// the huge-pool implementation reads.
///
/// The zeroed remainder is never exposed to allocator code that relies on the
/// full production `Segment::initialize` invariant.
fn boxed_huge_segment(raw: usize, block_size: usize) -> *mut Segment {
    // SAFETY: an all-zero bit pattern is a valid starting value for the
    // initializer to overwrite.
    let segment: *mut Segment = Box::into_raw(Box::new(unsafe { core::mem::zeroed() }));
    // SAFETY: `segment` is the live Box allocation just created.
    unsafe { Segment::initialize(segment, raw as *mut u8, 0) };
    // SAFETY: `segment` is the live Box allocation just created above, so
    // mutating its page-0 size metadata through the raw pointer is exclusive.
    unsafe {
        (*segment).pages[0].block_size = block_size as _;
    }
    segment
}

#[test]
fn test_huge_pool_exact_bucket_restores_rejected_head() {
    let pool = GlobalHugePool::new();
    // All four blocks land in bucket 1 ((16 KiB, 32 KiB]). Pushing the fitting
    // block first buries it at the bottom of the LIFO stack under three
    // undersized rejects (top-down order after the pushes: c, b, a, fitting).
    let fitting = boxed_huge_segment(0x20000, 24 * 1024);
    let small_a = boxed_huge_segment(0x30000, 17 * 1024);
    let small_b = boxed_huge_segment(0x40000, 18 * 1024);
    let small_c = boxed_huge_segment(0x50000, 19 * 1024);

    unsafe {
        assert!(pool.try_push(fitting, 0), "fitting segment must be cached");
        for small in [small_a, small_b, small_c] {
            assert!(
                pool.try_push(small, 0),
                "undersized same-bucket segment must be cached"
            );
        }
    }
    assert_eq!(pool.retained_blocks(), 4);
    assert_eq!(pool.retained_bytes(), (24 + 17 + 18 + 19) * 1024);

    let popped = unsafe { pool.pop(20 * 1024, 0) }
        .expect("same-size bucket must scan past undersized heads");
    assert_eq!(popped, fitting);
    unsafe {
        assert_eq!(
            (*popped)
                .next_free_segment
                .load(core::sync::atomic::Ordering::Relaxed),
            core::ptr::null_mut()
        );
    }

    // Count and byte conservation: exactly the three rejects remain cached.
    assert_eq!(pool.retained_blocks(), 3);
    assert_eq!(pool.retained_bytes(), (17 + 18 + 19) * 1024);

    // The rejected chain is spliced back in walk order, preserving the
    // original LIFO order (c, b, a): a request every reject satisfies must
    // pop them head-first in that exact order, proving each rejected segment
    // is still retrievable with intact size metadata and cleared links.
    for (expected, expected_size) in [
        (small_c, 19 * 1024),
        (small_b, 18 * 1024),
        (small_a, 17 * 1024),
    ] {
        let restored = unsafe { pool.pop(16 * 1024 + 1, 0) }
            .expect("rejected segment must be restored to the bucket");
        assert_eq!(restored, expected);
        unsafe {
            assert_eq!((*restored).pages[0].block_size as usize, expected_size);
            assert_eq!(
                (*restored)
                    .next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed),
                core::ptr::null_mut()
            );
        }
    }
    assert_eq!(pool.retained_blocks(), 0);
    assert_eq!(pool.retained_bytes(), 0);
    assert!(
        unsafe { pool.pop(16 * 1024, 0) }.is_none(),
        "no segment may remain after all rejects were drained"
    );

    for segment in [fitting, small_a, small_b, small_c] {
        unsafe {
            let _ = Box::from_raw(segment);
        }
    }
}

#[test]
fn test_huge_pool_admission_uses_actual_bucket_bytes() {
    use super::pool::huge_pool::huge_bucket_index;

    const BUCKET: usize = 9;
    const SMALL_BLOCK_SIZE: usize = 4 * 1024 * 1024 + 64 * 1024;
    const LARGE_BLOCK_SIZE: usize = 6 * 1024 * 1024 + 64 * 1024;

    assert_eq!(huge_bucket_index(SMALL_BLOCK_SIZE), BUCKET);
    assert_eq!(huge_bucket_index(LARGE_BLOCK_SIZE), BUCKET);

    let pool = GlobalHugePool::new();
    let small_count = GlobalHugePool::MAX_CACHED_HUGE_BYTES_PER_BAND / SMALL_BLOCK_SIZE;
    let mut small_segments = Vec::with_capacity(small_count);
    for index in 0..small_count {
        let segment = boxed_huge_segment(0x80000 + index * 0x1000, SMALL_BLOCK_SIZE);
        unsafe {
            assert!(
                pool.try_push(segment, 0),
                "small same-bucket segment {index} must be cached"
            );
        }
        small_segments.push(segment);
    }

    // This saturates the lower-band byte budget, but the upper-band budget
    // still admits the larger mapping. Partitioning the budget prevents a
    // common smaller mapping class from starving a neighboring reuse class.
    let large_segment = boxed_huge_segment(0x100000, LARGE_BLOCK_SIZE);
    unsafe {
        assert!(
            pool.try_push(large_segment, 0),
            "a larger mapping must fit while the byte budget has room"
        );
    }
    assert_eq!(pool.retained_blocks(), small_count + 1);
    assert_eq!(
        pool.retained_bytes(),
        small_count * SMALL_BLOCK_SIZE + LARGE_BLOCK_SIZE
    );

    let popped_large = unsafe { pool.pop(LARGE_BLOCK_SIZE, 0) }
        .expect("the larger same-bucket mapping must remain retrievable");
    assert_eq!(popped_large, large_segment);
    assert_eq!(pool.retained_blocks(), small_count);
    assert_eq!(pool.retained_bytes(), small_count * SMALL_BLOCK_SIZE);

    for expected in small_segments.into_iter().rev() {
        let popped = unsafe { pool.pop(SMALL_BLOCK_SIZE, 0) }
            .expect("every small mapping must remain retrievable");
        assert_eq!(popped, expected);
        unsafe {
            let _ = Box::from_raw(popped);
        }
    }
    unsafe {
        let _ = Box::from_raw(popped_large);
    }
    assert_eq!(pool.retained_blocks(), 0);
    assert_eq!(pool.retained_bytes(), 0);
}

#[test]
fn test_huge_pool_pop_skips_over_provisioned_buckets() {
    use super::pool::huge_pool::HUGE_POP_FIT_CAP;

    let pool = GlobalHugePool::new();
    // Ground the test's size choices in the cap: bucket 10's exclusive lower
    // bound (8 MiB) is beyond HUGE_POP_FIT_CAP x 20 KiB (inadmissible), while
    // bucket 2's (32 KiB) is within it (admissible).
    const {
        assert!(8 * 1024 * 1024 >= HUGE_POP_FIT_CAP * 20 * 1024);
        assert!(32 * 1024 < HUGE_POP_FIT_CAP * 20 * 1024);
    }

    // A cached 16 MiB block (bucket 10) must NOT satisfy a ~20 KiB-class
    // request (bucket 1): bucket 10's smallest possible block (8 MiB + 1)
    // exceeds HUGE_POP_FIT_CAP (4) x 20 KiB, so the scan stops long before it
    // and the pop misses instead of over-provisioning ~800x.
    let oversized = boxed_huge_segment(0x60000, 16 * 1024 * 1024);
    unsafe {
        assert!(pool.try_push(oversized, 0), "16 MiB block must be cached");
    }
    assert!(
        unsafe { pool.pop(20 * 1024, 0) }.is_none(),
        "a block beyond the fit cap must miss, not over-provision"
    );
    // The miss leaves the oversized block cached.
    assert_eq!(pool.retained_blocks(), 1);
    assert_eq!(pool.retained_bytes(), 16 * 1024 * 1024);

    // A higher bucket within the cap still hits: a 64 KiB block (bucket 2)
    // serves a 20 KiB request because bucket 2's lower bound (32 KiB) is
    // below 4 x 20 KiB = 80 KiB.
    let medium = boxed_huge_segment(0x70000, 64 * 1024);
    unsafe {
        assert!(pool.try_push(medium, 0), "64 KiB block must be cached");
    }
    let popped = unsafe { pool.pop(20 * 1024, 0) }
        .expect("a higher bucket within the fit cap must still serve the request");
    assert_eq!(popped, medium);
    assert_eq!(pool.retained_blocks(), 1);
    assert_eq!(pool.retained_bytes(), 16 * 1024 * 1024);

    // The oversized block itself is retrievable by a request it fits within
    // the cap (16 MiB request, exact bucket).
    let reclaimed = unsafe { pool.pop(16 * 1024 * 1024, 0) }
        .expect("exact-bucket request must retrieve the 16 MiB block");
    assert_eq!(reclaimed, oversized);
    assert_eq!(pool.retained_blocks(), 0);
    assert_eq!(pool.retained_bytes(), 0);

    for segment in [oversized, medium] {
        unsafe {
            let _ = Box::from_raw(segment);
        }
    }
}

#[test]
fn test_arena_stats_report_runtime_retained_cap() {
    use mnemosyne_core::options::{MnemosyneOptions, set_options};

    // Lower the runtime cap below the compile-time limit: the stat must track
    // the enforced runtime value (what `try_push_retained` reads), not the
    // compile-time `MAX_RETAINED_SEGMENTS_LIMIT`.
    set_options(MnemosyneOptions {
        max_retained_segments: 7,
        ..Default::default()
    });
    let stats = arena_memory_stats::<FailingReleaseBackend>();
    assert_eq!(stats.max_retained_free_segments, 7);

    // Restore the default; the stat must follow it. Compare against the
    // default option itself rather than the compile-time limit: the two are
    // equal in ordinary builds, but under Miri the default is deliberately 0
    // (options.rs suppresses retention so Miri's leak evidence stays focused
    // on live allocations). Asserting the constant encoded that build-specific
    // coincidence and made this test fail under Miri for no defect.
    set_options(MnemosyneOptions::default());
    let stats = arena_memory_stats::<FailingReleaseBackend>();
    assert_eq!(
        stats.max_retained_free_segments,
        MnemosyneOptions::default().max_retained_segments
    );
}

#[test]
fn test_arena_stats_track_huge_pool_blocks_and_bytes() {
    let before = arena_memory_stats::<FailingReleaseBackend>();

    let block = boxed_huge_segment(0x90000, 24 * 1024);
    unsafe {
        assert!(
            FailingReleaseBackend::global_huge_pool().try_push(block, 0),
            "huge block must be cached"
        );
    }
    let during = arena_memory_stats::<FailingReleaseBackend>();
    assert_eq!(during.retained_huge_blocks, before.retained_huge_blocks + 1);
    assert_eq!(
        during.retained_huge_bytes,
        before.retained_huge_bytes + 24 * 1024
    );

    let popped = unsafe { FailingReleaseBackend::global_huge_pool().pop(24 * 1024, 0) }
        .expect("cached huge block must be retrievable");
    assert_eq!(popped, block);
    let after = arena_memory_stats::<FailingReleaseBackend>();
    assert_eq!(after.retained_huge_blocks, before.retained_huge_blocks);
    assert_eq!(after.retained_huge_bytes, before.retained_huge_bytes);

    unsafe {
        let _ = Box::from_raw(block);
    }
}
