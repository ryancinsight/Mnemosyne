//! Aligned segment allocations from the OS or global pools.
//!
//! Returning a segment is the other direction and lives in
//! [`release`] beside this.

use mnemosyne_core::constants::{
    MAX_RETAINED_SEGMENTS_LIMIT, PAGE_SIZE, SEGMENT_ALIGN, SEGMENT_SIZE,
};

mod allocate;
mod release;

pub use allocate::allocate_segment;
pub(crate) use allocate::decommit_mapping_slack;
use allocate::try_return_to_pool;
pub use release::{
    deallocate_segment, purge_segment_pool, release_segment_mapping, reset_segment_pool,
    try_deallocate_segment,
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
/// guard size must not exceed the smallest supported OS page size. We
/// fix the value at 4 KiB, which is the system page size on every
/// supported Mnemosyne target (Linux/Windows/macOS-x86_64). On
/// platforms with a larger OS page size (macOS-arm64 at 16 KiB) the
/// underlying `mprotect`/`VirtualProtect` request will fail and the
/// guard install is silently skipped - the backend telemetry surfaces
/// the actual install count.
pub const SEGMENT_TAIL_GUARD_SIZE: usize = 4096;

const _: () = assert!(SEGMENT_TAIL_GUARD_SIZE.is_power_of_two());
const _: () = assert!(SEGMENT_TAIL_GUARD_SIZE <= SEGMENT_ALIGN);

/// Size of the guard region installed at the end of Page 0.
pub const SEGMENT_HEADER_GUARD_SIZE: usize = 4096;

const _: () = assert!(SEGMENT_HEADER_GUARD_SIZE.is_power_of_two());
const _: () = assert!(SEGMENT_HEADER_GUARD_SIZE <= PAGE_SIZE);
