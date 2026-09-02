//! Per-size-class allocation telemetry.
//!
//! Tracks process-wide allocation and deallocation counts per size class using
//! relaxed atomic counters.  The overhead per alloc/free is one `fetch_add` on
//! a per-class `AtomicU64`; on modern CPUs this is a single `LOCK XADD` with
//! no memory-ordering constraints (relaxed), which adds ≤ 1 ns to the fast
//! path.
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

// Three process-wide per-class atomic arrays.  Kept together so they fit on
// the same cache lines when iterated in `bin_stats`.
static ALLOC_COUNT:   [AtomicU64; NUM_SIZE_CLASSES] = [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];
static DEALLOC_COUNT: [AtomicU64; NUM_SIZE_CLASSES] = [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];
static ALLOC_BYTES:   [AtomicU64; NUM_SIZE_CLASSES] = [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];

/// Records one allocation from `class`.
///
/// `# Safety` is not required; the function bounds-checks `class` internally.
/// Called on the alloc hot path: one relaxed `fetch_add` per class array.
#[inline(always)]
pub(crate) fn record_alloc(class: usize) {
    if class < NUM_SIZE_CLASSES {
        ALLOC_COUNT[class].fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES[class].fetch_add(class_to_size(class) as u64, Ordering::Relaxed);
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
    let alloc_count   = ALLOC_COUNT[class].load(Ordering::Relaxed);
    let dealloc_count = DEALLOC_COUNT[class].load(Ordering::Relaxed);
    let alloc_bytes   = ALLOC_BYTES[class].load(Ordering::Relaxed);
    let block_size    = class_to_size(class);
    Some(BinSnapshot {
        alloc_count,
        dealloc_count,
        alloc_bytes,
        block_size,
        live_estimate: alloc_count.saturating_sub(dealloc_count),
    })
}

/// Returns snapshots for all `NUM_SIZE_CLASSES` size classes.
#[must_use]
pub fn all_bin_snapshots() -> [BinSnapshot; NUM_SIZE_CLASSES] {
    core::array::from_fn(|class| {
        let alloc_count   = ALLOC_COUNT[class].load(Ordering::Relaxed);
        let dealloc_count = DEALLOC_COUNT[class].load(Ordering::Relaxed);
        let alloc_bytes   = ALLOC_BYTES[class].load(Ordering::Relaxed);
        let block_size    = class_to_size(class);
        BinSnapshot {
            alloc_count,
            dealloc_count,
            alloc_bytes,
            block_size,
            live_estimate: alloc_count.saturating_sub(dealloc_count),
        }
    })
}
