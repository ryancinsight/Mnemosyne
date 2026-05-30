//! Shared arena and segment management logic for Mnemosyne.

#![no_std]

pub mod arena;
pub mod segment;

pub use arena::{allocate_large_or_huge, deallocate_large_or_huge};
pub use segment::{
    allocate_segment, arena_memory_stats, checked_align_up, deallocate_segment, purge_segment_pool,
    reset_segment_pool, ArenaMemoryStats, GlobalSegmentPool, HasSegmentPool, MAX_RETAINED_SEGMENTS,
    SEGMENT_MAPPING_SIZE,
};
