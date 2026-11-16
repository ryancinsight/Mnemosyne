//! Huge-pool tests: bucket indexing, bucket fan-out, and exact-bucket
//! restoration of rejected heads.

use super::pool::GlobalHugePool;
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
pub(super) fn boxed_huge_segment(raw: usize, block_size: usize) -> *mut Segment {
    // SAFETY: an all-zero bit pattern is a valid starting value for the
    // initializer to overwrite.
    let segment: *mut Segment = Box::into_raw(Box::new(unsafe { core::mem::zeroed() }));
    // SAFETY: `segment` is the live Box allocation just created.
    unsafe { Segment::initialize(segment, core::ptr::without_provenance_mut(raw), 0) };
    // SAFETY: `segment` is the live Box allocation just created above, so
    // mutating its page-0 size metadata through the raw pointer is exclusive.
    unsafe {
        (*segment).pages[0].block_size = u32::try_from(block_size)
            .expect("invariant: huge-pool test block sizes fit the page-0 u32 field");
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

    // SAFETY: each pushed segment is a live, initialized Box allocation
    // from `boxed_huge_segment` that this test exclusively owns until the
    // matching `pop` or the final `Box::from_raw`.
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

    // SAFETY: `pop` hands the segment to this test as sole owner; it stays
    // a live Box allocation until the final `Box::from_raw`.
    let popped = unsafe { pool.pop(20 * 1024, 0) }
        .expect("same-size bucket must scan past undersized heads");
    assert_eq!(popped, fitting);
    // SAFETY: `popped` is the live Box allocation of `fitting`, owned by
    // this test, so reading its link field is a plain atomic load.
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
        // SAFETY: `pop` returns a segment this test pushed and again owns
        // exclusively; it stays a live Box allocation until `Box::from_raw`.
        let restored = unsafe { pool.pop(16 * 1024 + 1, 0) }
            .expect("rejected segment must be restored to the bucket");
        assert_eq!(restored, expected);
        // SAFETY: `restored` equals `expected`, a live Box allocation owned
        // by this test, so reading its metadata and link field is sound.
        unsafe {
            assert_eq!((*restored).pages[0].block_size, expected_size);
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
    // SAFETY: the pool is empty, so `pop` returns `None` and transfers no
    // segment.
    assert!(
        unsafe { pool.pop(16 * 1024, 0) }.is_none(),
        "no segment may remain after all rejects were drained"
    );

    for segment in [fitting, small_a, small_b, small_c] {
        // SAFETY: each segment came from `Box::into_raw` in
        // `boxed_huge_segment` and the pool no longer holds it, so this is
        // the single reclaiming `Box::from_raw`.
        unsafe {
            let _ = Box::from_raw(segment);
        }
    }
}
