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

impl BinSnapshot {
    /// Allocation fragmentation ratio for this class: the fraction of
    /// allocated bytes that are currently not returned, i.e.
    /// `live_estimate * block_size / alloc_bytes` clamped to `[0.0, 1.0]`.
    ///
    /// Returns `0.0` when `alloc_bytes == 0` (nothing ever allocated in this
    /// class).
    #[inline]
    #[must_use]
    pub fn fragmentation_ratio(&self) -> f64 {
        if self.alloc_bytes == 0 {
            return 0.0;
        }
        let live_bytes = self.live_estimate.saturating_mul(self.block_size as u64);
        (live_bytes as f64 / self.alloc_bytes as f64).min(1.0)
    }
}

/// Returns the index of the hottest size class (highest `alloc_count`), or
/// `None` when no allocations have ever been made.
#[inline]
#[must_use]
pub fn hottest_class() -> Option<usize> {
    let snapshots = all_bin_snapshots();
    snapshots
        .iter()
        .enumerate()
        .max_by_key(|(_, s)| s.alloc_count)
        .and_then(|(idx, s)| if s.alloc_count > 0 { Some(idx) } else { None })
}

/// Process-wide live bytes in the small allocator: sum of
/// `live_estimate × block_size` across all classes.
///
/// Like all bin-stats aggregates this is a best-effort snapshot; the
/// individual counters are not captured atomically, so the result can
/// temporarily under-estimate during concurrent allocation bursts.
#[inline]
#[must_use]
pub fn total_live_bytes() -> u64 {
    let snapshots = all_bin_snapshots();
    snapshots
        .iter()
        .map(|s| s.live_estimate.saturating_mul(s.block_size as u64))
        .fold(0u64, u64::saturating_add)
}

/// Process-wide total allocation count across all small size classes.
#[inline]
#[must_use]
pub fn total_alloc_count() -> u64 {
    ALLOC_COUNT
        .iter()
        .map(|c| c.load(Ordering::Relaxed))
        .fold(0u64, u64::saturating_add)
}

/// Returns a one-line human-readable summary of process-wide bin stats, e.g.
/// `"allocs=1234 live_bytes=65536 hottest_class=5(48b)"`.
///
/// The format is intended for diagnostic logging; do not rely on it for
/// machine parsing (it may change across versions).
#[must_use]
pub fn summary_line() -> std::string::String {
    let total_allocs = total_alloc_count();
    let live = total_live_bytes();
    match hottest_class() {
        Some(cls) => {
            let block = class_to_size(cls);
            std::format!(
                "allocs={total_allocs} live_bytes={live} hottest_class={cls}({block}b)"
            )
        }
        None => std::format!("allocs={total_allocs} live_bytes={live} hottest_class=none"),
    }
}
