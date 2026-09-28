//! Arithmetic fallback for `size_to_class` used to build [`super::SIZE_TO_CLASS`].
//!
//! This function is `const` and evaluated entirely at compile time; it is the
//! SSOT for the piecewise class-step schedule. The lookup table in the parent
//! module calls it once per granule during initialization and caches the result.

use crate::constants::MAX_SMALL_ALLOC_SIZE;
#[derive(Clone, Copy)]
struct SizeClassLookup {
    base: u8,
    shift: u8,
    sub: u16,
}

#[inline(always)]
const fn lookup(base: u8, shift: u8, sub: u16) -> SizeClassLookup {
    SizeClassLookup { base, shift, sub }
}

const STEP_16: SizeClassLookup = lookup(0, 4, 1);
const STEP_32: SizeClassLookup = lookup(8, 5, 129);
const STEP_128: SizeClassLookup = lookup(20, 7, 513);
const STEP_512: SizeClassLookup = lookup(32, 9, 2049);
const STEP_1024: SizeClassLookup = lookup(44, 10, 8193);

pub(super) const fn size_to_class_nonzero_arithmetic(size: usize) -> Option<usize> {
    if size > MAX_SMALL_ALLOC_SIZE {
        return None;
    }

    const LOOKUP: [SizeClassLookup; 15] = [
        STEP_16,   // idx = 0 (size = 0, fallback)
        STEP_16,   // idx = 1
        STEP_16,   // idx = 2
        STEP_16,   // idx = 3
        STEP_16,   // idx = 4
        STEP_16,   // idx = 5
        STEP_16,   // idx = 6
        STEP_16,   // idx = 7
        STEP_32,   // idx = 8
        STEP_32,   // idx = 9
        STEP_128,  // idx = 10
        STEP_128,  // idx = 11
        STEP_512,  // idx = 12
        STEP_512,  // idx = 13
        STEP_1024, // idx = 14: 8193..=16384 in 1024-byte steps (MN-REF-1: was shift=11 / 2048-byte steps)
    ];

    let bits = usize::BITS - (size - 1).leading_zeros();
    if bits >= LOOKUP.len() as u32 {
        return None;
    }
    let entry = &LOOKUP[bits as usize];
    Some(entry.base as usize + ((size - entry.sub as usize) >> entry.shift))
}
