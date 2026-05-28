//! Layout constants for the Mnemosyne memory allocator.

/// The size of a segment (2MB).
pub const SEGMENT_SIZE: usize = 2 * 1024 * 1024;

/// The alignment of a segment (2MB).
pub const SEGMENT_ALIGN: usize = SEGMENT_SIZE;

/// The size of a page (64KB).
pub const PAGE_SIZE: usize = 64 * 1024;

/// The alignment of a page (64KB).
pub const PAGE_ALIGN: usize = PAGE_SIZE;

/// The number of pages per segment (32).
pub const PAGES_PER_SEGMENT: usize = SEGMENT_SIZE / PAGE_SIZE;

/// The maximum size of a small allocation class (8KB).
pub const MAX_SMALL_ALLOC_SIZE: usize = 8 * 1024;

/// Maximum single allocation payload size accepted by public allocation entry points.
///
/// This mirrors Rust `Layout`'s pointer-offset safety bound: allocated object
/// sizes must not exceed `isize::MAX`.
pub const MAX_ALLOC_SIZE: usize = isize::MAX as usize;

/// The total number of small size classes.
pub const NUM_SIZE_CLASSES: usize = 44;
