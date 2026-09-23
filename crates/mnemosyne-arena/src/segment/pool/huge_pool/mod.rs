//! Retained huge mappings, bucketed by size class within each NUMA node.
//!
//! The pool is decomposed by responsibility so each leaf owns one path:
//! - [`bucket`] — the SSOT logarithmic size-to-bucket geometry and the
//!   over-provision cap that bound cache reuse.
//! - [`push`] — [`GlobalHugePool::try_push`] admission into a node bucket.
//! - [`pop`] — [`GlobalHugePool::pop`] retrieval with local-first stealing.
//! - [`stats`] — advisory retained-block/byte counts and the snapshot.
//! - [`purge`] — reclamation of every bucket back to the OS.
//!
//! `mod.rs` keeps only the shared `GlobalHugePool` struct shape, its budget
//! constants, and the `new` constructor that the concern submodules extend
//! through additional `impl` blocks.

mod bucket;
mod pop;
mod purge;
mod push;
mod stats;

// The bucket types stay reachable at their original path: this module is
// `pub`, so moving them out of the file would have removed
// `pool::huge_pool::NodeHugeBucket` from the public surface — a break the
// semver gate flagged, and not one a file-size refactor may make.
pub use super::node_huge_bucket::{NodeHugeBucket, NodeHugePool};
// The bucketing SSOT lives in `bucket`. `HUGE_SIZE_BUCKETS` is consumed in
// production by `node_huge_bucket` via this module's path, so it is
// re-exported unconditionally. `huge_bucket_index` / `HUGE_POP_FIT_CAP` are
// referenced only by the segment tests through `pool::huge_pool::<name>`
// (production push/pop import them straight from `bucket`), so their
// re-export is test-only to avoid an unused-import warning in library builds.
pub(crate) use bucket::HUGE_SIZE_BUCKETS;
#[cfg(test)]
pub(crate) use bucket::{HUGE_POP_FIT_CAP, huge_bucket_index};

use super::numa_bucket::NUMA_BUCKETS;

/// A NUMA-aware reclamation-safe global pool of free huge allocations.
pub struct GlobalHugePool {
    nodes: [NodeHugePool; NUMA_BUCKETS],
}

impl Default for GlobalHugePool {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalHugePool {
    /// Bounded maximum number of huge allocations cached per NUMA node bucket.
    pub const MAX_CACHED_HUGE_BLOCKS: usize = 1024;
    /// Maximum size class we cache (16MB).
    pub const MAX_CACHED_HUGE_SIZE: usize = 16 * 1024 * 1024;
    /// Per-bucket retained-byte budget. Admission enforces this budget against
    /// actual block sizes, so mixed-size buckets use available capacity without
    /// allowing a single bucket to retain more than ~256 MiB of idle mappings.
    pub const MAX_CACHED_HUGE_BYTES_PER_BUCKET: usize = 256 * 1024 * 1024;
    /// Per-band retained-byte budget. The two bands partition the bucket budget
    /// so a saturated lower band cannot starve upper-band reuse.
    pub(crate) const MAX_CACHED_HUGE_BYTES_PER_BAND: usize =
        Self::MAX_CACHED_HUGE_BYTES_PER_BUCKET / super::node_huge_bucket::HUGE_BUCKET_BANDS;

    /// Creates a new empty `GlobalHugePool` with `NUMA_BUCKETS` node sub-pools.
    pub const fn new() -> Self {
        // Derive the array length from the `NUMA_BUCKETS` SSOT rather than a
        // hand-written literal, so the fan-out can never drift from the constant.
        Self {
            nodes: [const { NodeHugePool::new() }; NUMA_BUCKETS],
        }
    }
}
