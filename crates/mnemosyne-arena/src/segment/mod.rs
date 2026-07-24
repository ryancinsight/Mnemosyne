//! Aligned segment allocations from the OS or global pools.

mod alignment;
pub mod alloc;
pub mod pool;
pub mod stats;
#[cfg(test)]
pub mod tests;

pub use alignment::checked_align_up;
pub use alloc::{
    MAX_RETAINED_SEGMENTS, SEGMENT_MAPPING_SIZE, allocate_segment, deallocate_segment,
    purge_segment_pool, release_segment_mapping, reset_segment_pool,
};
pub use pool::{GlobalHugePool, GlobalSegmentPool, HasSegmentPool};
pub use stats::{ArenaMemoryStats, SegmentRelease, arena_memory_stats};
