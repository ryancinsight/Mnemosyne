//! Size class calculations and mapping.

use crate::constants::{MAX_SMALL_ALLOC_SIZE, NUM_SIZE_CLASSES};

/// Maps an allocation size to its corresponding size class index.
///
/// Returns `None` if the size exceeds `MAX_SMALL_ALLOC_SIZE`. A size of `0`
/// maps to class `0` because the production allocation entry points reject
/// zero-size requests before reaching this function (`is_valid_alloc_request`
/// and `is_valid_layout_alloc_request` both require `size != 0`), but the
/// historical mapping is preserved so callers that pass an already-adjusted
/// minimum size still resolve to the smallest class without an extra branch.
#[inline]
pub const fn size_to_class(size: usize) -> Option<usize> {
    if size == 0 {
        return Some(0);
    }
    if size > MAX_SMALL_ALLOC_SIZE {
        return None;
    }

    if size <= 128 {
        Some((size - 1) / 16)
    } else if size <= 512 {
        Some(((size - 129) / 32) + 8)
    } else if size <= 2048 {
        Some(((size - 513) / 128) + 20)
    } else {
        Some(((size - 2049) / 512) + 32)
    }
}

/// Maps a size class index to its maximum block size.
///
/// Returns `0` if the class index is out of bounds (>= `NUM_SIZE_CLASSES`).
#[inline]
pub const fn class_to_size(class: usize) -> usize {
    if class < 8 {
        (class + 1) * 16
    } else if class < 20 {
        128 + (class - 7) * 32
    } else if class < 32 {
        512 + (class - 19) * 128
    } else if class < NUM_SIZE_CLASSES {
        2048 + (class - 31) * 512
    } else {
        0
    }
}

// Compile-time cross-check between `NUM_SIZE_CLASSES` and the piecewise
// `class_to_size` schedule: the final class must produce exactly
// `MAX_SMALL_ALLOC_SIZE`, and the first out-of-range class must produce
// the documented zero sentinel.
const _: () = assert!(
    class_to_size(NUM_SIZE_CLASSES - 1) == MAX_SMALL_ALLOC_SIZE,
    "class_to_size(NUM_SIZE_CLASSES - 1) must reach MAX_SMALL_ALLOC_SIZE exactly"
);
const _: () = assert!(
    class_to_size(NUM_SIZE_CLASSES) == 0,
    "class_to_size(NUM_SIZE_CLASSES) must return the 0 sentinel"
);

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(size_to_class(8193), None);

        for c in 0..NUM_SIZE_CLASSES {
            let sz = class_to_size(c);
            assert!(sz > 0, "class_to_size({c}) returned zero");
            assert_eq!(size_to_class(sz), Some(c));
        }
    }

    #[test]
    fn size_class_boundaries_are_exact() {
        // Walk every consecutive class pair: the byte immediately after a
        // class's upper bound must map to the next class, and the upper
        // bound itself must map to the class. Catches off-by-one errors at
        // the four piecewise transitions in `size_to_class`: 128/129,
        // 512/513, 2048/2049, and 8192/8193.
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
                // Past the final class, every larger size must spill into
                // the large/huge arena routing.
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
        // The production validators reject zero-size requests before they
        // reach the size-class mapper, but the mapper's documented zero
        // behavior is part of its contract and is exercised whenever a
        // caller passes an already-adjusted minimum size.
        assert_eq!(size_to_class(0), Some(0));
        // The smallest non-zero size also maps to class 0.
        assert_eq!(size_to_class(1), Some(0));
    }
}
