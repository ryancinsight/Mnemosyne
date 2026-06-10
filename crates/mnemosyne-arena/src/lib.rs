//! Shared arena and segment management logic for Mnemosyne.

#![no_std]

extern crate alloc;

pub mod arena;
pub mod numa;
pub mod scratch;
pub mod segment;

pub use arena::{allocate_large_or_huge, deallocate_large_or_huge};
pub use numa::current_numa_node;
pub use scratch::{AlignedVec, ScratchBank, ScratchElement, ScratchPool, DEFAULT_SCRATCH_ALIGN};
pub use segment::{
    allocate_segment, arena_memory_stats, checked_align_up, deallocate_segment, purge_segment_pool,
    reset_segment_pool, ArenaMemoryStats, GlobalHugePool, GlobalSegmentPool, HasSegmentPool,
    MAX_RETAINED_SEGMENTS, SEGMENT_MAPPING_SIZE,
};
