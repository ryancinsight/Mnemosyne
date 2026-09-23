//! Size class calculations and mapping.
//!
//! The module is organized by concern:
//! - [`tables`] — SSOT compile-time lookup tables (`CLASS_TO_SIZE`,
//!   `CLASS_TO_MAX_BLOCKS`, `CLASS_TO_DIV_MULT`, `SIZE_TO_CLASS`) and their
//!   O(1) direct accessors (`class_to_size`, `class_to_max_blocks`,
//!   `block_index_in_page`).
//! - [`arithmetic`] — the `const fn` piecewise bucketing math that drives the
//!   reverse table at compile time (`size_to_class_nonzero_arithmetic`).
//! - [`info`] — `SizeClassInfo` compile-time metadata struct.
//!
//! This file owns the query functions that combine the reverse and forward
//! tables: `size_to_class*`, `round_up_size*`, `size_class_fragmentation`.

mod arithmetic;
mod info;
pub mod tables;

pub use info::{SizeClassInfo, all_class_info};
pub use tables::{LEMIRE_DIV_SHIFT, block_index_in_page, class_to_max_blocks, class_to_size};

use crate::constants::{MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE};
use tables::SIZE_TO_CLASS;

/// Maps an allocation size to its corresponding size class index.
///
/// Returns `None` if the size exceeds `MAX_SMALL_ALLOC_SIZE`. A size of `0`
/// maps to class `0` because the production allocation entry points reject
/// zero-size requests before reaching this function (`is_valid_alloc_request`
/// and `is_valid_layout_alloc_request` both require `size != 0`), but the
/// historical mapping is preserved so callers that pass an already-adjusted
/// minimum size still resolve to the smallest class without an extra branch.
#[inline(always)]
pub const fn size_to_class(size: usize) -> Option<usize> {
    if size == 0 {
        return Some(0);
    }
    size_to_class_nonzero(size)
}

/// Maps a non-zero allocation size to its corresponding size class index.
#[inline(always)]
pub const fn size_to_class_nonzero(size: usize) -> Option<usize> {
    if size > MAX_SMALL_ALLOC_SIZE {
        return None;
    }
    // Every class size is a multiple of `MIN_BLOCK_SIZE`, so a request rounded
    // up to the next multiple of 16 lands in the same class as the request
    // itself. Indexing by that 16-byte granule keeps the table at one entry
    // per granule (1 KiB at a 16 KiB ceiling) instead of one per byte.
    let class = SIZE_TO_CLASS[size.div_ceil(MIN_BLOCK_SIZE)];
    if class == u8::MAX {
        None
    } else {
        Some(class as usize)
    }
}

/// Returns the rounded size-class block size for a given allocation size.
///
/// Equivalent to `size_to_class(size).map(class_to_size)`.
#[inline(always)]
pub const fn round_up_size(size: usize) -> Option<usize> {
    if size == 0 {
        return Some(0);
    }
    if size > MAX_SMALL_ALLOC_SIZE {
        return None;
    }
    match size_to_class_nonzero(size) {
        Some(class) => Some(class_to_size(class)),
        None => None,
    }
}

/// Returns the class stride for the given request, saturating to the largest
/// class when `size > MAX_SMALL_ALLOC_SIZE`.
///
/// This is the `round_up_size` variant that never returns `None`:
/// - `size == 0` → `0`
/// - `size > MAX_SMALL_ALLOC_SIZE` → `MAX_SMALL_ALLOC_SIZE`
///
/// Useful when you need an aligned stride without separately handling the
/// large-allocation path.
#[inline(always)]
pub const fn round_up_size_saturating(size: usize) -> usize {
    if size == 0 {
        return 0;
    }
    match round_up_size(size) {
        Some(s) => s,
        None => MAX_SMALL_ALLOC_SIZE,
    }
}

/// Returns the internal fragmentation for a request of `size` bytes:
/// `(class_stride - size) / class_stride`, in `[0.0, 1.0]`.
///
/// Returns `0.0` when `size == 0` or `size > MAX_SMALL_ALLOC_SIZE`.
#[inline]
pub fn size_class_fragmentation(size: usize) -> f64 {
    if size == 0 {
        return 0.0;
    }
    let Some(stride) = round_up_size(size) else {
        return 0.0;
    };
    if stride == 0 {
        return 0.0;
    }
    let waste = stride.saturating_sub(size);
    waste as f64 / stride as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::NUM_SIZE_CLASSES;

    #[test]
    fn test_size_class_mapping() {
        assert_eq!(size_to_class(0), Some(0));
        assert_eq!(size_to_class(16), Some(0));
        assert_eq!(size_to_class(17), Some(1));
        assert_eq!(size_to_class(128), Some(7));
        assert_eq!(size_to_class(129), Some(8));
        assert_eq!(size_to_class(160), Some(8));
        assert_eq!(size_to_class(512), Some(19));
        assert_eq!(size_to_class(513), Some(20));
        assert_eq!(size_to_class(2048), Some(31));
        assert_eq!(size_to_class(2049), Some(32));
        assert_eq!(size_to_class(8192), Some(43));
        // MN-REF-1: 8K-16K now uses 1024-byte steps instead of 2048-byte steps.
        assert_eq!(size_to_class(8193), Some(44));
        assert_eq!(size_to_class(9216), Some(44));
        assert_eq!(size_to_class(9217), Some(45));
        assert_eq!(size_to_class(10240), Some(45));
        assert_eq!(size_to_class(10241), Some(46));
        assert_eq!(size_to_class(11264), Some(46));
        assert_eq!(size_to_class(11265), Some(47));
        assert_eq!(size_to_class(12288), Some(47));
        assert_eq!(size_to_class(12289), Some(48));
        assert_eq!(size_to_class(13312), Some(48));
        assert_eq!(size_to_class(13313), Some(49));
        assert_eq!(size_to_class(14336), Some(49));
        assert_eq!(size_to_class(14337), Some(50));
        assert_eq!(size_to_class(15360), Some(50));
        assert_eq!(size_to_class(15361), Some(51));
        assert_eq!(size_to_class(16384), Some(51));
        assert_eq!(size_to_class(16385), None);

        for c in 0..NUM_SIZE_CLASSES {
            let sz = class_to_size(c);
            assert!(sz > 0, "class_to_size({c}) returned zero");
            assert_eq!(size_to_class(sz), Some(c));
        }
    }

    #[test]
    fn size_class_boundaries_are_exact() {
        for c in 0..NUM_SIZE_CLASSES {
            let upper = class_to_size(c);
            assert_eq!(
                size_to_class(upper),
                Some(c),
                "class {c} upper bound {upper} must resolve to {c}"
            );
            if c + 1 < NUM_SIZE_CLASSES {
                assert_eq!(
                    size_to_class(upper + 1),
                    Some(c + 1),
                    "class {} lower bound {} must resolve to {}",
                    c + 1,
                    upper + 1,
                    c + 1
                );
            } else {
                assert_eq!(
                    size_to_class(upper + 1),
                    None,
                    "byte past final class must escape small routing"
                );
            }
        }
    }

    #[test]
    fn size_class_zero_maps_to_smallest_class() {
        assert_eq!(size_to_class(0), Some(0));
        assert_eq!(size_to_class(1), Some(0));
    }

    #[test]
    fn block_index_in_page_matches_integer_division() {
        for class in 0..NUM_SIZE_CLASSES {
            let block_size = class_to_size(class);
            let max_blocks = class_to_max_blocks(class);
            for idx in 0..max_blocks {
                let offset = idx * block_size;
                let expected = offset / block_size;
                let fast = block_index_in_page(class, offset);
                assert_eq!(
                    fast, expected,
                    "class={class} block_size={block_size} offset={offset}: \
                     lemire={fast} != div={expected}"
                );
            }
        }
    }

    #[test]
    fn round_up_size_saturating_never_returns_zero_for_positive() {
        for sz in [
            1usize,
            16,
            17,
            100,
            512,
            2048,
            MAX_SMALL_ALLOC_SIZE,
            MAX_SMALL_ALLOC_SIZE + 1,
        ] {
            let result = round_up_size_saturating(sz);
            if sz == 0 {
                assert_eq!(result, 0);
            } else {
                assert!(result > 0, "round_up_size_saturating({sz}) returned 0");
            }
        }
    }
}
