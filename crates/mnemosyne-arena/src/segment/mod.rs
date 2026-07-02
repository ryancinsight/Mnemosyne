//! Aligned segment allocations from the OS or global pools.

pub mod alloc;
pub mod pool;
pub mod stats;
#[cfg(test)]
pub mod tests;
pub mod utils;

pub use alloc::{
    MAX_RETAINED_SEGMENTS, SEGMENT_MAPPING_SIZE, allocate_segment, deallocate_segment,
    purge_segment_pool, release_segment_mapping, reset_segment_pool,
};
pub use pool::{GlobalHugePool, GlobalSegmentPool, HasSegmentPool};
pub use stats::{ArenaMemoryStats, SegmentRelease, arena_memory_stats};
pub use utils::checked_align_up;
