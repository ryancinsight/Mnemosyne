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

/// Constructs an [`AlignedVec`] from a literal list of elements.
///
/// Syntax mirrors `vec!` from the standard library:
/// - `aligned_vec![1u32, 2, 3]` — create from elements
/// - `aligned_vec![0u8; 64]` — fill with `n` copies of a value
///
/// # Examples
///
/// ```rust
/// use mnemosyne_arena::aligned_vec;
/// let v = aligned_vec![1u32, 2, 3];
/// assert_eq!(v.as_slice(), &[1, 2, 3]);
///
/// let zeros = aligned_vec![0u8; 8];
/// assert_eq!(zeros.len(), 8);
/// assert!(zeros.iter().all(|&b| b == 0));
/// ```
#[macro_export]
macro_rules! aligned_vec {
    () => {
        $crate::AlignedVec::dangling()
    };
    ($elem:expr; $n:expr) => {
        $crate::AlignedVec::filled($n, $elem)
    };
    ($($x:expr),+ $(,)?) => {{
        let slice: &[_] = &[$($x),+];
        $crate::AlignedVec::from_slice(slice)
    }};
}
