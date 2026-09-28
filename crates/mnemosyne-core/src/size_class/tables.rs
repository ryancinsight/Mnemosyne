//! Compile-time lookup tables for the size-class layout and their direct
//! O(1) accessors.
//!
//! This module is the **SSOT** for all class-layout data (each table below
//! is `pub(super)`, visible to the query functions but not part of the
//! public API):
//! - `CLASS_TO_SIZE` — block stride per class.
//! - `CLASS_TO_MAX_BLOCKS` — page capacity per class.
//! - `CLASS_TO_DIV_MULT` — Lemire reciprocal multipliers.
//! - `SIZE_TO_CLASS` — granule-indexed reverse lookup (class per 16-byte granule).
//!
//! The companion query functions in [`super`] (`size_to_class`,
//! `round_up_size`, …) call these; nothing else should index the raw arrays.

use crate::constants::{MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, NUM_SIZE_CLASSES, PAGE_SIZE};

// ── Forward (class → size) table ──────────────────────────────────────────

pub(super) const CLASS_TO_SIZE: [u16; NUM_SIZE_CLASSES] = [
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

pub(super) const CLASS_TO_MAX_BLOCKS: [u16; NUM_SIZE_CLASSES] = {
    let mut arr = [0u16; NUM_SIZE_CLASSES];
    let mut i = 0;
    while i < NUM_SIZE_CLASSES {
        arr[i] = (PAGE_SIZE / CLASS_TO_SIZE[i] as usize) as u16;
        i += 1;
    }
    arr
};

// ── Reverse (granule → class) table ───────────────────────────────────────

/// Class per 16-byte granule of request size: entry `g` serves every request
/// in `(16 * (g - 1), 16 * g]`, and entry 0 the zero-size request.
pub(super) const SIZE_TO_CLASS: [u8; MAX_SMALL_ALLOC_SIZE / MIN_BLOCK_SIZE + 1] = {
    use super::arithmetic::size_to_class_nonzero_arithmetic;
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

// ── Lemire reciprocal division ─────────────────────────────────────────────
//
// Pre-computed `div_mult` per class eliminates the hardware division in
// `block_index = offset / block_size` on the validation path.
//
// Algorithm (Lemire indirect, 32-bit shift):
//   mult = (2^SHIFT / n) + 1  →  index = (offset * mult) >> SHIFT
//
// Correctness bounds: offset < PAGE_SIZE (≤65535), n ≥ 16.  The round-up
// error in `mult` never propagates for these bounds.
//
// Inspired by snmalloc `ds/sizeclasstable.h §slab_index`.

/// Shift constant for the Lemire indirect reciprocal.
pub const LEMIRE_DIV_SHIFT: u32 = 32;

pub(super) const CLASS_TO_DIV_MULT: [u32; NUM_SIZE_CLASSES] = {
    let mut arr = [0u32; NUM_SIZE_CLASSES];
    let mut i = 0;
    while i < NUM_SIZE_CLASSES {
        let n = CLASS_TO_SIZE[i] as u64;
        arr[i] = ((1u64 << LEMIRE_DIV_SHIFT) / n + 1) as u32;
        i += 1;
    }
    arr
};

// ── O(1) accessors ─────────────────────────────────────────────────────────

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

/// Block index within a page using Lemire reciprocal multiplication.
///
/// Equivalent to `offset / class_to_size(class)` without a division
/// instruction. `offset` must be `< PAGE_SIZE`; `class` must be
/// `< NUM_SIZE_CLASSES`.
#[inline(always)]
pub const fn block_index_in_page(class: usize, offset: usize) -> usize {
    ((offset as u64 * CLASS_TO_DIV_MULT[class] as u64) >> LEMIRE_DIV_SHIFT) as usize
}

// ── Structural invariant checks ────────────────────────────────────────────
//
// Verify the CLASS_TO_SIZE table properties that prevent subtle bugs when the
// schedule is extended:
// 1. Strict monotone: each class is strictly larger than the previous.
// 2. Min block: the smallest class is MIN_BLOCK_SIZE (16 bytes).
// 3. Alignment divisibility: every size is a multiple of MIN_BLOCK_SIZE.
// 4. Every class fits in a page: class_to_max_blocks(class) >= 1.

const _: () = {
    assert!(
        CLASS_TO_SIZE[0] as usize == MIN_BLOCK_SIZE,
        "smallest size class must equal MIN_BLOCK_SIZE"
    );
    let mut i = 1;
    while i < NUM_SIZE_CLASSES {
        assert!(
            CLASS_TO_SIZE[i] > CLASS_TO_SIZE[i - 1],
            "CLASS_TO_SIZE must be strictly increasing"
        );
        assert!(
            (CLASS_TO_SIZE[i] as usize).is_multiple_of(MIN_BLOCK_SIZE),
            "every size class must be a multiple of MIN_BLOCK_SIZE"
        );
        assert!(
            CLASS_TO_MAX_BLOCKS[i] >= 1,
            "every size class must fit at least one block per page"
        );
        i += 1;
    }
};
