//! Arithmetic fallback for `size_to_class` used to build [`super::SIZE_TO_CLASS`].
//!
//! This function is `const` and evaluated entirely at compile time; it is the
//! SSOT for the piecewise class-step schedule. The lookup table in the parent
//! module calls it once per granule during initialization and caches the result.

use crate::constants::{MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE};
pub(super) const fn size_to_class_nonzero_arithmetic(size: usize) -> Option<usize> {
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
