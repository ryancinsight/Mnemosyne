//! Size class calculations and mapping.

use crate::constants::{MAX_SMALL_ALLOC_SIZE, NUM_SIZE_CLASSES};

/// Maps an allocation size to its corresponding size class index.
///
/// Returns `None` if the size exceeds `MAX_SMALL_ALLOC_SIZE`.
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
            assert!(sz > 0);
            assert_eq!(size_to_class(sz), Some(c));
        }
    }
}
