//! Layout constants for the Mnemosyne memory allocator.

/// The size of a segment (2MB).
pub const SEGMENT_SIZE: usize = 2 * 1024 * 1024;

/// The alignment of a segment (2MB).
pub const SEGMENT_ALIGN: usize = SEGMENT_SIZE;

/// The size of a page (64KB).
pub const PAGE_SIZE: usize = 64 * 1024;

/// The shift amount corresponding to PAGE_SIZE (e.g. 16 for 64KB).
pub const PAGE_SHIFT: usize = PAGE_SIZE.trailing_zeros() as usize;

/// The alignment of a page (64KB).
pub const PAGE_ALIGN: usize = PAGE_SIZE;

/// The number of pages per segment (32).
pub const PAGES_PER_SEGMENT: usize = SEGMENT_SIZE / PAGE_SIZE;

/// The maximum size of a small allocation class (8KB).
pub const MAX_SMALL_ALLOC_SIZE: usize = 8 * 1024;

/// The smallest size-class block, in bytes.
///
/// This is the allocation fast-path alignment ceiling: requests whose
/// alignment is no larger than this value can be served from small pages.
pub const MIN_BLOCK_SIZE: usize = 16;

/// Maximum single allocation payload size accepted by public allocation entry points.
///
/// This mirrors Rust `Layout`'s pointer-offset safety bound: allocated object
/// sizes must not exceed `isize::MAX`.
pub const MAX_ALLOC_SIZE: usize = isize::MAX as usize;

/// The total number of small size classes.
pub const NUM_SIZE_CLASSES: usize = 44;

// Compile-time structural invariant checks.
//
// These `const _: () = assert!(...)` items are evaluated by the compiler
// before any code is generated, so any constant drift that breaks an
// allocator-wide layout assumption produces a hard build failure rather
// than a silent runtime fault. They cost zero bytes and zero instructions.

/// `SEGMENT_SIZE` and `PAGE_SIZE` must be powers of two so the bitmask-based
/// address rounding (`addr & !(SEGMENT_SIZE - 1)`, `addr & !(PAGE_SIZE - 1)`)
/// produces the correct base address.
const _: () = assert!(
    SEGMENT_SIZE.is_power_of_two(),
    "SEGMENT_SIZE must be a power of two for bitmask address rounding"
);
const _: () = assert!(
    PAGE_SIZE.is_power_of_two(),
    "PAGE_SIZE must be a power of two for bitmask address rounding"
);

/// `SEGMENT_ALIGN` is referenced as both an alignment cap (rejecting larger
/// alignments in `is_valid_alloc_request`) and as the rounding modulus for
/// page-to-segment recovery. The two roles must agree on the same value.
const _: () = assert!(
    SEGMENT_ALIGN == SEGMENT_SIZE,
    "SEGMENT_ALIGN must equal SEGMENT_SIZE so the small-free classifier can recover the segment header by rounding"
);
const _: () = assert!(
    PAGE_ALIGN == PAGE_SIZE,
    "PAGE_ALIGN must equal PAGE_SIZE so page-base derivation matches the page array stride"
);

/// `PAGES_PER_SEGMENT * PAGE_SIZE == SEGMENT_SIZE` is the array-stride
/// derivation used by `Segment::initialize` and the small-free classifier.
const _: () = assert!(
    PAGES_PER_SEGMENT * PAGE_SIZE == SEGMENT_SIZE,
    "PAGES_PER_SEGMENT must tile SEGMENT_SIZE exactly with PAGE_SIZE strides"
);

/// At least one page must be available for small allocations after Page 0
/// is reserved for segment metadata.
const _: () = assert!(
    PAGES_PER_SEGMENT >= 2,
    "PAGES_PER_SEGMENT must reserve Page 0 for metadata and leave at least one slicing page"
);

/// Every small allocation must fit inside a single page so `Page::initialize_free_list`
/// can lay out at least one block of every size class.
const _: () = assert!(
    MAX_SMALL_ALLOC_SIZE <= PAGE_SIZE,
    "MAX_SMALL_ALLOC_SIZE must fit inside one PAGE_SIZE"
);

/// `MAX_ALLOC_SIZE` is the public payload bound; it must be at least one
/// segment so `allocate_large_or_huge` can serve any request the
/// small-allocation path rejects.
const _: () = assert!(
    MAX_ALLOC_SIZE >= SEGMENT_SIZE,
    "MAX_ALLOC_SIZE must accept at least one segment-sized payload"
);

/// `NUM_SIZE_CLASSES` is the array dimension for per-class metadata
/// (`active_pages`, `full_pages`, `size_class_occupancy`); it must be
/// non-zero for the arrays to hold any state.
const _: () = assert!(
    NUM_SIZE_CLASSES > 0,
    "NUM_SIZE_CLASSES must be non-zero so per-class allocator arrays hold at least one entry"
);

/// `MIN_BLOCK_SIZE` must divide `PAGE_SIZE` exactly so the densest page is
/// fully tiled with no trailing partial block, keeping the block-count
/// derivation above exact.
const _: () = assert!(
    PAGE_SIZE % MIN_BLOCK_SIZE == 0,
    "MIN_BLOCK_SIZE must divide PAGE_SIZE exactly"
);
