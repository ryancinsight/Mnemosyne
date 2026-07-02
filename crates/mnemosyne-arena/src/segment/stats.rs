//! Arena memory telemetry statistics types and helpers.

use super::alloc::SEGMENT_MAPPING_SIZE;
use super::pool::HasSegmentPool;

/// Snapshot of arena-level segment cache state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ArenaMemoryStats {
    pub retained_free_segments: usize,
    /// Runtime retention cap currently enforced by the segment pool
    /// (`mnemosyne_core::options::MAX_RETAINED_SEGMENTS`, clamped at set time
    /// to the compile-time `MAX_RETAINED_SEGMENTS_LIMIT`), not the
    /// compile-time limit itself.
    pub max_retained_free_segments: usize,
    pub retained_free_bytes: usize,
    pub purged_segments: usize,
    pub purge_calls: usize,
    pub purged_bytes: usize,
    /// Number of segments whose physical backing was released by a
    /// confirmed `page_reset` while the segment itself remained cached
    /// in the retained pool.
    pub reset_segments: usize,
    /// Number of `reset_segment_pool` invocations.
    pub reset_calls: usize,
    /// Number of huge blocks currently retained in the huge-allocation cache
    /// across all NUMA nodes.
    pub retained_huge_blocks: usize,
    /// Total bytes of huge blocks currently retained in the huge-allocation
    /// cache across all NUMA nodes — typically the dominant share of retained
    /// RSS.
    pub retained_huge_bytes: usize,
}

/// Outcome of attempting to release a segment mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentRelease {
    /// The backend confirmed release of the OS mapping.
    Released,
    /// The backend reported release failure; ownership remains with the pool.
    RetainedAfterFailure,
}

/// Returns the current arena segment cache counters.
#[inline]
pub fn arena_memory_stats<B: HasSegmentPool>() -> ArenaMemoryStats {
    let pool = B::global_segment_pool();
    let huge_pool = B::global_huge_pool();
    let retained = pool.retained_count();
    ArenaMemoryStats {
        retained_free_segments: retained,
        // The runtime option is the enforced cap (`try_push_retained` reads it
        // per push); the compile-time limit is only its clamp ceiling.
        max_retained_free_segments: mnemosyne_core::options::MAX_RETAINED_SEGMENTS
            .load(core::sync::atomic::Ordering::Relaxed),
        retained_free_bytes: retained * SEGMENT_MAPPING_SIZE,
        purged_segments: pool.purged_count(),
        purge_calls: pool.purge_call_count(),
        purged_bytes: pool.purged_count() * SEGMENT_MAPPING_SIZE,
        reset_segments: pool.reset_segments_count(),
        reset_calls: pool.reset_call_count(),
        retained_huge_blocks: huge_pool.retained_blocks(),
        retained_huge_bytes: huge_pool.retained_bytes(),
    }
}
