//! Size-to-bucket geometry for the huge pool: the SSOT logarithmic bucketing
//! math and the over-provision cap that bound cache reuse.
//!
//! [`HUGE_SIZE_BUCKETS`] is derived here from
//! [`GlobalHugePool::MAX_CACHED_HUGE_SIZE`] so the fan-out can never drift
//! from the cacheable range; every consumer of the bucket layout
//! ([`super::super::node_huge_bucket`], the push/pop paths) reads that one
//! definition.

use super::super::node_huge_bucket::HugeBucketBand;
use super::GlobalHugePool;

/// Number of huge size buckets: the bucket index of the largest cacheable size
/// ([`GlobalHugePool::MAX_CACHED_HUGE_SIZE`]) plus one.
///
/// `try_push` rejects anything larger than `MAX_CACHED_HUGE_SIZE`, so buckets
/// beyond that index would be permanently unreachable dead statics (and wasted
/// count-line reads on every pop miss). Deriving the count from the SSOT pins
/// the fan-out to the cacheable range; the const assertion below enforces that
/// the max cacheable size maps to the last bucket.
pub(crate) const HUGE_SIZE_BUCKETS: usize =
    log2_ceil_bucket_index(GlobalHugePool::MAX_CACHED_HUGE_SIZE) + 1;

/// Unclamped log2-ceil bucket index: sizes `<= 16 KiB` map to bucket 0;
/// otherwise bucket `b` covers `(2^(b+13), 2^(b+14)]` bytes.
///
/// This is the raw bucketing math that also defines [`HUGE_SIZE_BUCKETS`];
/// callers use [`huge_bucket_index`], which clamps to the live bucket range.
const fn log2_ceil_bucket_index(size: usize) -> usize {
    if size <= 16384 {
        0
    } else {
        let bits = usize::BITS - (size - 1).leading_zeros();
        (bits as usize).saturating_sub(14)
    }
}

#[inline(always)]
pub(crate) const fn huge_bucket_index(size: usize) -> usize {
    let idx = log2_ceil_bucket_index(size);
    if idx >= HUGE_SIZE_BUCKETS {
        HUGE_SIZE_BUCKETS - 1
    } else {
        idx
    }
}

/// Selects the ordered half of a logarithmic bucket for `size`.
///
/// The lower band ends at the bucket midpoint. A request in the upper band
/// cannot be satisfied by any lower-band mapping, so exact-bucket lookup can
/// skip that stack instead of walking and restoring known-undersized blocks.
#[inline(always)]
pub(super) const fn huge_bucket_band(size: usize, bucket_idx: usize) -> HugeBucketBand {
    let lower_bound = 1usize << (bucket_idx + 13);
    let midpoint = lower_bound + lower_bound / 2;
    if size > midpoint {
        HugeBucketBand::Upper
    } else {
        HugeBucketBand::Lower
    }
}

// Pin the SSOT derivation: the largest cacheable size maps to the last bucket,
// so exactly `huge_bucket_index(MAX_CACHED_HUGE_SIZE) + 1` buckets are live.
const _: () =
    assert!(huge_bucket_index(GlobalHugePool::MAX_CACHED_HUGE_SIZE) == HUGE_SIZE_BUCKETS - 1);

/// Upward-scan over-provision cap factor for cache pops.
///
/// `pop_from_node` serves a request from a bucket above the request's own only
/// while that bucket's smallest possible block (`2^(bucket_idx+13) + 1` bytes —
/// bucket `b` covers `(2^(b+13), 2^(b+14)]`) does not exceed
/// `HUGE_POP_FIT_CAP ×` the requested total size. Because a bucket's largest
/// block is less than 2× its exclusive lower bound, a cache hit then
/// over-provisions the request by less than `2 × HUGE_POP_FIT_CAP = 8×` in the
/// worst case, while still permitting reuse across adjacent size classes.
/// Without the cap, a ~20 KiB-class request could be satisfied by a cached
/// 16 MiB block (~800× over-provision) whose slack stays committed, because
/// the cache-hit allocation path skips slack decommit. Buckets beyond the cap
/// are skipped without popping: the bucket index lower-bounds every block a
/// bucket holds, so none of them can satisfy the cap.
pub(crate) const HUGE_POP_FIT_CAP: usize = 4;
