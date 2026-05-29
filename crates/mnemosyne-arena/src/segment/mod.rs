//! Aligned segment allocations from the OS or global pools.

pub mod alloc;
pub mod pool;
pub mod stats;
pub mod tests;
pub mod utils;

pub use alloc::{
    allocate_segment, deallocate_segment, purge_segment_pool, release_segment_mapping,
    reset_segment_pool, MAX_RETAINED_SEGMENTS, SEGMENT_MAPPING_SIZE,
};
pub use pool::{GlobalSegmentPool, HasSegmentPool};
pub use stats::{arena_memory_stats, ArenaMemoryStats, SegmentRelease};
pub use utils::checked_align_up;
