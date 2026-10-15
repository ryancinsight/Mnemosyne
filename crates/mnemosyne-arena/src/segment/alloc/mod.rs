//! Aligned segment allocations from the OS or global pools.
//!
//! Returning a segment is the other direction and lives in the adjacent
//! release module.

use mnemosyne_core::constants::{
    MAX_RETAINED_SEGMENTS_LIMIT, PAGE_SIZE, SEGMENT_ALIGN, SEGMENT_SIZE,
};

mod allocate;
#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
pub(super) mod guard;
mod release;

pub(crate) use allocate::decommit_mapping_slack;
use allocate::try_return_to_pool;
pub use allocate::{AcquiredSegment, acquire_segment, allocate_segment};
pub use release::{
    deallocate_segment, purge_segment_pool, purge_segment_pool_with_warm, release_segment_mapping,
    reset_segment_pool, try_deallocate_segment,
};

/// Bytes requested from the OS for each standard segment mapping.
pub const SEGMENT_MAPPING_SIZE: usize = SEGMENT_SIZE * 2;

/// Free segment mappings retained for reuse.
pub const MAX_RETAINED_SEGMENTS: usize = MAX_RETAINED_SEGMENTS_LIMIT;

/// Size of the guard region installed in the slack after every segment.
///
/// The guard lives at `aligned_addr + SEGMENT_SIZE`, inside the
/// `SEGMENT_MAPPING_SIZE - SEGMENT_SIZE` of address-space slack the
/// arena reserves to satisfy `SEGMENT_ALIGN` rounding. Worst-case
/// available slack-after = `OS_PAGE_SIZE` (when the raw OS mapping
/// happened to be aligned to `SEGMENT_ALIGN - OS_PAGE_SIZE`), so the
/// minimum is 4 KiB. The actual request covers at least one whole native OS
/// page, and never exceeds the reserved mapping. On 16-KiB-page systems this
/// requests 16 KiB. Unsupported geometry or a kernel-declined request leaves
/// this best-effort guard inactive; telemetry records confirmed requests only.
pub const SEGMENT_TAIL_GUARD_SIZE: usize = 4096;

const _: () = assert!(SEGMENT_TAIL_GUARD_SIZE.is_power_of_two());
const _: () = assert!(SEGMENT_TAIL_GUARD_SIZE <= SEGMENT_ALIGN);

/// Minimum guard size at the end of Page 0, rounded to whole native OS pages.
/// A header guard is omitted if it would overlap the live Segment metadata.
pub const SEGMENT_HEADER_GUARD_SIZE: usize = 4096;

const _: () = assert!(SEGMENT_HEADER_GUARD_SIZE.is_power_of_two());
const _: () = assert!(SEGMENT_HEADER_GUARD_SIZE <= PAGE_SIZE);
