//! Per-size-class allocation telemetry.
//!
//! Tracks process-wide allocation and deallocation counts per size class using
//! relaxed atomic counters. The allocation-byte total is derived from the
//! immutable class stride, so recording an allocation needs one atomic update
//! instead of a second counter RMW on the hot path.
//!
//! The counters are always enabled (no feature gate).  They are not
//! synchronised with each other — a snapshot can observe counts from
//! different points in time — but they are individually monotone-non-decreasing,
//! so the live-allocation estimate `alloc_count - dealloc_count` is always an
//! under-estimate rather than a negative artefact.
//!
//! Fragmentation ratio per class: `(alloc_bytes - dealloc_bytes) /
//! alloc_bytes`.  Internal fragmentation per class: `(alloc_bytes -
//! requested_bytes) / alloc_bytes`.

use core::sync::atomic::{AtomicU64, Ordering};
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use mnemosyne_core::size_class::class_to_size;

// Two process-wide per-class atomic arrays. Allocation bytes are derived from
// the immutable class stride when a snapshot is read.
static ALLOC_COUNT: [AtomicU64; NUM_SIZE_CLASSES] = [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];
static DEALLOC_COUNT: [AtomicU64; NUM_SIZE_CLASSES] =
    [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];

#[inline]
fn allocation_bytes(alloc_count: u64, block_size: usize) -> u64 {
    alloc_count.saturating_mul(block_size as u64)
}

/// Records one allocation from `class`.
///
/// `# Safety` is not required; the function bounds-checks `class` internally.
/// Called on the alloc hot path: one relaxed `fetch_add` for the class count.
#[inline(always)]
pub(crate) fn record_alloc(class: usize) {
    if class < NUM_SIZE_CLASSES {
        ALLOC_COUNT[class].fetch_add(1, Ordering::Relaxed);
    }
}

/// Records one deallocation into `class`.
#[inline(always)]
pub(crate) fn record_dealloc(class: usize) {
    if class < NUM_SIZE_CLASSES {
        DEALLOC_COUNT[class].fetch_add(1, Ordering::Relaxed);
    }
}

/// Per-size-class allocation statistics snapshot.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BinSnapshot {
    /// Total allocations served from this size class.
    pub alloc_count: u64,
    /// Total frees returned to this size class.
    pub dealloc_count: u64,
    /// Cumulative bytes allocated (size-class block size × alloc_count).
    ///
    /// The product saturates at `u64::MAX` rather than wrapping.
    pub alloc_bytes: u64,
    /// Block size of this size class in bytes.
    pub block_size: usize,
    /// Live allocation estimate: `alloc_count − dealloc_count`.
    ///
    /// Under-estimates because the two counters are not snapshotted
    /// atomically, but never negative from the caller's perspective:
    /// subtraction uses saturating arithmetic.
    pub live_estimate: u64,
}

/// Returns a snapshot for size class `class`, or `None` if out of range.
#[must_use]
pub fn bin_snapshot(class: usize) -> Option<BinSnapshot> {
    if class >= NUM_SIZE_CLASSES {
        return None;
    }
    let alloc_count = ALLOC_COUNT[class].load(Ordering::Relaxed);
    let dealloc_count = DEALLOC_COUNT[class].load(Ordering::Relaxed);
    let block_size = class_to_size(class);
    Some(BinSnapshot {
        alloc_count,
        dealloc_count,
        alloc_bytes: allocation_bytes(alloc_count, block_size),
        block_size,
        live_estimate: alloc_count.saturating_sub(dealloc_count),
    })
}

/// Returns snapshots for all `NUM_SIZE_CLASSES` size classes.
#[must_use]
pub fn all_bin_snapshots() -> [BinSnapshot; NUM_SIZE_CLASSES] {
    core::array::from_fn(|class| {
        let alloc_count = ALLOC_COUNT[class].load(Ordering::Relaxed);
        let dealloc_count = DEALLOC_COUNT[class].load(Ordering::Relaxed);
        let block_size = class_to_size(class);
        BinSnapshot {
            alloc_count,
            dealloc_count,
            alloc_bytes: allocation_bytes(alloc_count, block_size),
            block_size,
            live_estimate: alloc_count.saturating_sub(dealloc_count),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{NUM_SIZE_CLASSES, all_bin_snapshots, bin_snapshot};

    #[test]
    fn derived_allocation_bytes_match_the_class_stride() {
        for snapshot in all_bin_snapshots() {
            assert_eq!(
                snapshot.alloc_bytes,
                snapshot
                    .alloc_count
                    .saturating_mul(snapshot.block_size as u64),
                "allocation bytes must be derived from the immutable class stride"
            );
        }
    }

    #[test]
    fn snapshots_preserve_the_public_range_contract() {
        assert!(bin_snapshot(NUM_SIZE_CLASSES).is_none());
    }
}
