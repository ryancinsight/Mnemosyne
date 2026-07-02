//! Shared arena and segment management logic for Mnemosyne.

#![cfg_attr(not(any(feature = "std", test)), no_std)]

#[cfg(any(feature = "std", test))]
extern crate std;

extern crate alloc;

pub mod arena;
pub mod numa;
pub mod scratch;
pub mod segment;

pub use arena::{allocate_large_or_huge, deallocate_large_or_huge};
pub use numa::current_numa_node;
pub use scratch::{AlignedVec, DEFAULT_SCRATCH_ALIGN, ScratchBank, ScratchElement, ScratchPool};
pub use segment::{
    ArenaMemoryStats, GlobalHugePool, GlobalSegmentPool, HasSegmentPool, MAX_RETAINED_SEGMENTS,
    SEGMENT_MAPPING_SIZE, allocate_segment, arena_memory_stats, checked_align_up,
    deallocate_segment, purge_segment_pool, reset_segment_pool,
};
