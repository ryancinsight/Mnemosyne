//! [SizeClassInfo] — a compile-time metadata bundle for one size class.
//!
//! Bundles lock_size, max_blocks, Lemire div_mult, and class index
//! from the individual lookup tables in [super], so a consumer that needs
//! more than one field per class makes only one call.

use super::tables::{CLASS_TO_DIV_MULT, CLASS_TO_MAX_BLOCKS, CLASS_TO_SIZE, LEMIRE_DIV_SHIFT};
use super::{class_to_size, size_to_class};
use crate::constants::{MAX_SMALL_ALLOC_SIZE, NUM_SIZE_CLASSES};
///
/// Bundles all the per-class constants that are computed separately in the
/// individual lookup functions — convenient when a consumer needs multiple
/// fields for the same class without repeated calls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SizeClassInfo {
    /// Block size in bytes for this class.
    pub block_size: usize,
    /// Maximum blocks per 64 KiB page.
    pub max_blocks: usize,
    /// Lemire reciprocal multiplier for `block_index_in_page`.
    pub div_mult: u32,
    /// 0-based class index.
    pub class: usize,
}

impl SizeClassInfo {
    /// Returns the metadata for class `class`, or `None` if out of range.
    #[must_use]
    pub const fn for_class(class: usize) -> Option<Self> {
        if class >= NUM_SIZE_CLASSES {
            return None;
        }
        Some(Self {
            block_size: CLASS_TO_SIZE[class] as usize,
            max_blocks: CLASS_TO_MAX_BLOCKS[class] as usize,
            div_mult: CLASS_TO_DIV_MULT[class],
            class,
        })
    }

    /// Returns the metadata for the class that serves `size`, or `None`
    /// when `size > MAX_SMALL_ALLOC_SIZE`.
    #[must_use]
    #[inline]
    pub const fn for_size(size: usize) -> Option<Self> {
        match size_to_class(size) {
            Some(class) => Self::for_class(class),
            None => None,
        }
    }

    /// Computes the block index for a page offset using the pre-computed
    /// Lemire multiplier.
    #[inline(always)]
    pub const fn block_index(&self, offset: usize) -> usize {
        ((offset as u64 * self.div_mult as u64) >> LEMIRE_DIV_SHIFT) as usize
    }

    /// Internal fragmentation for a request of `requested` bytes served by
    /// this class: `(block_size - requested) / block_size`, in `[0.0, 1.0]`.
    #[inline]
    pub fn fragmentation_for(&self, requested: usize) -> f64 {
        if self.block_size == 0 || requested >= self.block_size {
            return 0.0;
        }
        (self.block_size - requested) as f64 / self.block_size as f64
    }
}

/// Returns an iterator-compatible array of [`SizeClassInfo`] for all classes.
#[must_use]
pub fn all_class_info() -> [SizeClassInfo; NUM_SIZE_CLASSES] {
    core::array::from_fn(|class| SizeClassInfo::for_class(class).expect("class in range"))
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

// ── Size class table ordering invariants ────────────────────────────────────
