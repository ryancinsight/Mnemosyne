//! Shared arena and segment management logic for Mnemosyne.

#![cfg_attr(not(any(feature = "std", test)), no_std)]
#![deny(missing_docs)]

#[cfg(any(feature = "std", test))]
extern crate std;

extern crate alloc;

pub mod arena;
pub mod numa;
pub mod scratch;
pub mod segment;

pub use arena::{allocate_large_or_huge, deallocate_large_or_huge};
pub use numa::{bind_segment_to_numa_node, current_numa_node};
pub use scratch::{
    AlignedVec, DEFAULT_SCRATCH_ALIGN, Drain, IntoIter, ScratchBank, ScratchElement, ScratchPool,
};
pub use segment::{
    ArenaMemoryStats, GlobalHugePool, GlobalSegmentPool, HasSegmentPool, HugePoolStats,
    MAX_RETAINED_SEGMENTS, SEGMENT_MAPPING_SIZE, SegmentPoolStats, allocate_segment,
    arena_memory_stats, checked_align_up, deallocate_segment, purge_segment_pool,
    purge_segment_pool_with_warm, reset_segment_pool, try_deallocate_segment,
};
