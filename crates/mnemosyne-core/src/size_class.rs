//! Size class calculations and mapping.

use crate::constants::{MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, NUM_SIZE_CLASSES, PAGE_SIZE};

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

/// Class per 16-byte granule of request size: entry `g` serves every request
/// in `(16 * (g - 1), 16 * g]`, and entry 0 the zero-size request.
const SIZE_TO_CLASS: [u8; MAX_SMALL_ALLOC_SIZE / MIN_BLOCK_SIZE + 1] = {
    let mut arr = [u8::MAX; MAX_SMALL_ALLOC_SIZE / MIN_BLOCK_SIZE + 1];
    arr[0] = 0;
    let mut granule = 1;
    while granule <= MAX_SMALL_ALLOC_SIZE / MIN_BLOCK_SIZE {
        arr[granule] = match size_to_class_nonzero_arithmetic(granule * MIN_BLOCK_SIZE) {
            Some(class) => class as u8,
            None => u8::MAX,
        };
        granule += 1;
    }
    arr
};

const fn size_to_class_nonzero_arithmetic(size: usize) -> Option<usize> {
    if size > MAX_SMALL_ALLOC_SIZE {
        return None;
    }
    struct SizeClassLookup {
        base: u8,
        shift: u8,
        sub: u16,
    }

    const LOOKUP: [SizeClassLookup; 15] = [
        SizeClassLookup {
            base: 0,
            shift: 4,
            sub: 1,
        }, // idx = 0 (size = 0, fallback)
        SizeClassLookup {
            base: 0,
            shift: 4,
            sub: 1,
        }, // idx = 1
        SizeClassLookup {
            base: 0,
            shift: 4,
            sub: 1,
        }, // idx = 2
        SizeClassLookup {
            base: 0,
            shift: 4,
            sub: 1,
        }, // idx = 3
        SizeClassLookup {
            base: 0,
            shift: 4,
            sub: 1,
        }, // idx = 4
        SizeClassLookup {
            base: 0,
            shift: 4,
            sub: 1,
        }, // idx = 5
        SizeClassLookup {
            base: 0,
            shift: 4,
            sub: 1,
        }, // idx = 6
        SizeClassLookup {
            base: 0,
            shift: 4,
            sub: 1,
        }, // idx = 7
        SizeClassLookup {
            base: 8,
            shift: 5,
            sub: 129,
        }, // idx = 8
        SizeClassLookup {
            base: 8,
            shift: 5,
            sub: 129,
        }, // idx = 9
        SizeClassLookup {
            base: 20,
            shift: 7,
            sub: 513,
        }, // idx = 10
        SizeClassLookup {
            base: 20,
            shift: 7,
            sub: 513,
        }, // idx = 11
        SizeClassLookup {
            base: 32,
            shift: 9,
            sub: 2049,
        }, // idx = 12
        SizeClassLookup {
            base: 32,
            shift: 9,
            sub: 2049,
        }, // idx = 13
        SizeClassLookup {
            base: 44,
            shift: 10,
            sub: 8193,
        }, // idx = 14: 8193..=16384 in 1024-byte steps (MN-REF-1: was shift=11 / 2048-byte steps)
    ];

    let bits = usize::BITS - (size - 1).leading_zeros();
    if bits >= LOOKUP.len() as u32 {
        return None;
    }
    let entry = &LOOKUP[bits as usize];
    Some(entry.base as usize + ((size - entry.sub as usize) >> entry.shift))
}

/// Returns the rounded size-class block size for a given allocation size.
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

const CLASS_TO_SIZE: [u16; NUM_SIZE_CLASSES] = [
    // 16–128 bytes: 16-byte steps (classes 0–7)
    16, 32, 48, 64, 80, 96, 112, 128, // 129–512 bytes: 32-byte steps (classes 8–19)
    160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 480, 512,
    // 513–2048 bytes: 128-byte steps (classes 20–31)
    640, 768, 896, 1024, 1152, 1280, 1408, 1536, 1664, 1792, 1920, 2048,
    // 2049–8192 bytes: 512-byte steps (classes 32–43)
    2560, 3072, 3584, 4096, 4608, 5120, 5632, 6144, 6656, 7168, 7680, 8192,
    // 8193–16384 bytes: 1024-byte steps (classes 44–51)
    // MN-REF-1: was 2048-byte steps (4 classes); now 1024-byte steps (8 classes).
    // Reduces worst-case internal fragmentation in the 8–16 KB band from 25% to 12.5%.
    9216, 10240, 11264, 12288, 13312, 14336, 15360, 16384,
];

const CLASS_TO_MAX_BLOCKS: [u16; NUM_SIZE_CLASSES] = {
    let mut arr = [0u16; NUM_SIZE_CLASSES];
    let mut i = 0;
    while i < NUM_SIZE_CLASSES {
        arr[i] = (PAGE_SIZE / CLASS_TO_SIZE[i] as usize) as u16;
        i += 1;
    }
    arr
};

/// Maps a size class index to its maximum block size.
///
/// Returns `0` if the class index is out of bounds (>= `NUM_SIZE_CLASSES`).
#[inline(always)]
pub const fn class_to_size(class: usize) -> usize {
    if class < NUM_SIZE_CLASSES {
        CLASS_TO_SIZE[class] as usize
    } else {
        0
    }
}

/// Maps a size class index to its maximum number of blocks in a page.
///
/// Returns `0` if the class index is out of bounds (>= `NUM_SIZE_CLASSES`).
#[inline(always)]
pub const fn class_to_max_blocks(class: usize) -> usize {
    if class < NUM_SIZE_CLASSES {
        CLASS_TO_MAX_BLOCKS[class] as usize
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
        // MN-REF-1: 8K–16K now uses 1024-byte steps instead of 2048-byte steps.
        assert_eq!(size_to_class(8193), Some(44)); // class 44 = 9216
        assert_eq!(size_to_class(9216), Some(44));
        assert_eq!(size_to_class(9217), Some(45)); // class 45 = 10240
        assert_eq!(size_to_class(10240), Some(45));
        assert_eq!(size_to_class(10241), Some(46)); // class 46 = 11264
        assert_eq!(size_to_class(11264), Some(46));
        assert_eq!(size_to_class(11265), Some(47)); // class 47 = 12288
        assert_eq!(size_to_class(12288), Some(47));
        assert_eq!(size_to_class(12289), Some(48)); // class 48 = 13312
        assert_eq!(size_to_class(13312), Some(48));
        assert_eq!(size_to_class(13313), Some(49)); // class 49 = 14336
        assert_eq!(size_to_class(14336), Some(49));
        assert_eq!(size_to_class(14337), Some(50)); // class 50 = 15360
        assert_eq!(size_to_class(15360), Some(50));
        assert_eq!(size_to_class(15361), Some(51)); // class 51 = 16384
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
        // Walk every consecutive class pair: the byte immediately after a
        // class's upper bound must map to the next class, and the upper
        // bound itself must map to the class. Catches off-by-one errors at
        // the four piecewise transitions in `size_to_class`: 128/129,
        // 512/513, 2048/2049, 8192/8193, and 16384/16385.
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
