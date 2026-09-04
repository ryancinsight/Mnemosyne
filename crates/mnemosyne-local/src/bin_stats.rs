//! Per-size-class allocation telemetry.
//!
//! Tracks process-wide allocation and deallocation counts per size class using
//! relaxed atomic counters. The hot path uses **thread-local accumulators** that
//! flush to the global atomics every `FLUSH_THRESHOLD` operations per class,
//! reducing `LOCK XADD` overhead by up to 64× under multi-threaded workloads.
//!
//! The counters are always enabled (no feature gate). They are not
//! synchronised with each other — a snapshot can observe counts from
//! different points in time — but they are individually monotone-non-decreasing,
//! so the live-allocation estimate `alloc_count - dealloc_count` always
//! under-estimates rather than producing a negative artefact.
//!
//! **Thread-exit note**: thread-local accumulators are not flushed at thread
//! exit (no destructor), so a small under-count (≤ `FLUSH_THRESHOLD` per class
//! per dead thread) may occur. This is intentional and acceptable for telemetry.

use core::sync::atomic::{AtomicU64, Ordering};
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use mnemosyne_core::size_class::class_to_size;

// Three process-wide per-class atomic arrays.
static ALLOC_COUNT: [AtomicU64; NUM_SIZE_CLASSES] = [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];
static DEALLOC_COUNT: [AtomicU64; NUM_SIZE_CLASSES] =
    [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];
static ALLOC_BYTES: [AtomicU64; NUM_SIZE_CLASSES] = [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];

// ── Thread-local accumulator ──────────────────────────────────────────────────

/// How many per-class operations to accumulate before flushing to the global
/// atomic counters. Higher values reduce lock pressure at the cost of a
/// slightly coarser telemetry resolution.
const FLUSH_THRESHOLD: u32 = 64;

/// Per-thread bin counter accumulator.
///
/// Stores accumulated counts as `u32`; at `FLUSH_THRESHOLD` the batch is
/// added to the corresponding `AtomicU64` global with a single `LOCK XADD`.
#[derive(Clone, Copy)]
struct TlsBinAccumulator {
    alloc_count: [u32; NUM_SIZE_CLASSES],
    dealloc_count: [u32; NUM_SIZE_CLASSES],
}

impl TlsBinAccumulator {
    const fn zeroed() -> Self {
        Self {
            alloc_count: [0; NUM_SIZE_CLASSES],
            dealloc_count: [0; NUM_SIZE_CLASSES],
        }
    }
}

std::thread_local! {
    // `Cell<T>` gives O(1) get/set without any borrow-flag overhead for
    // the common single-threaded TLS access pattern.  We replace the whole
    // struct on flush rather than modifying it through a reference so that
    // `Cell` — not `RefCell` — suffices, removing the borrow-check branch.
    static TLS: std::cell::Cell<TlsBinAccumulator> =
        const { std::cell::Cell::new(TlsBinAccumulator::zeroed()) };
}

#[cold]
#[inline(never)]
fn flush_alloc(class: usize, accumulated: u32) {
    ALLOC_COUNT[class].fetch_add(accumulated as u64, Ordering::Relaxed);
    ALLOC_BYTES[class].fetch_add(
        accumulated as u64 * class_to_size(class) as u64,
        Ordering::Relaxed,
    );
}

#[cold]
#[inline(never)]
fn flush_dealloc(class: usize, accumulated: u32) {
    DEALLOC_COUNT[class].fetch_add(accumulated as u64, Ordering::Relaxed);
}

/// Records one allocation from `class`.
///
/// Bounds-checked; out-of-range class indices are silently ignored.
/// Hot path: one thread-local read-modify-write; atomic `LOCK XADD` only
/// every `FLUSH_THRESHOLD` allocations per class.
#[inline(always)]
pub(crate) fn record_alloc(class: usize) {
    if class < NUM_SIZE_CLASSES {
        TLS.with(|cell| {
            let mut acc = cell.get();
            let n = acc.alloc_count[class].wrapping_add(1);
            if n >= FLUSH_THRESHOLD {
                acc.alloc_count[class] = 0;
                cell.set(acc);
                flush_alloc(class, n);
            } else {
                acc.alloc_count[class] = n;
                cell.set(acc);
            }
        });
    }
}

/// Records one deallocation into `class`.
///
/// Hot path: thread-local accumulation with atomic flush every
/// `FLUSH_THRESHOLD` deallocations per class.
#[inline(always)]
pub(crate) fn record_dealloc(class: usize) {
    if class < NUM_SIZE_CLASSES {
        TLS.with(|cell| {
            let mut acc = cell.get();
            let n = acc.dealloc_count[class].wrapping_add(1);
            if n >= FLUSH_THRESHOLD {
                acc.dealloc_count[class] = 0;
                cell.set(acc);
                flush_dealloc(class, n);
            } else {
                acc.dealloc_count[class] = n;
                cell.set(acc);
            }
        });
    }
}

/// Flushes the calling thread's accumulator to the global counters immediately.
///
/// Useful before reading a stats snapshot on the same thread so that the
/// snapshot reflects all allocations made on this thread, not just those that
/// already exceeded `FLUSH_THRESHOLD`.
#[inline]
pub fn flush_tls_stats() {
    TLS.with(|cell| {
        let acc = cell.get();
        for class in 0..NUM_SIZE_CLASSES {
            let a = acc.alloc_count[class];
            if a > 0 {
                ALLOC_COUNT[class].fetch_add(a as u64, Ordering::Relaxed);
                ALLOC_BYTES[class]
                    .fetch_add(a as u64 * class_to_size(class) as u64, Ordering::Relaxed);
            }
            let d = acc.dealloc_count[class];
            if d > 0 {
                DEALLOC_COUNT[class].fetch_add(d as u64, Ordering::Relaxed);
            }
        }
        // Reset the accumulator so we don't double-count on the next flush.
        cell.set(TlsBinAccumulator::zeroed());
    });
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

impl BinSnapshot {
    /// Allocation fragmentation ratio for this class:
    /// `live_estimate * block_size / alloc_bytes`, clamped to `[0.0, 1.0]`.
    ///
    /// Returns `0.0` when `alloc_bytes == 0` (nothing ever allocated).
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

/// Returns a snapshot for size class `class`, or `None` if out of range.
#[must_use]
pub fn bin_snapshot(class: usize) -> Option<BinSnapshot> {
    if class >= NUM_SIZE_CLASSES {
        return None;
    }
    let alloc_count = ALLOC_COUNT[class].load(Ordering::Relaxed);
    let dealloc_count = DEALLOC_COUNT[class].load(Ordering::Relaxed);
    let alloc_bytes = ALLOC_BYTES[class].load(Ordering::Relaxed);
    let block_size = class_to_size(class);
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
        let alloc_count = ALLOC_COUNT[class].load(Ordering::Relaxed);
        let dealloc_count = DEALLOC_COUNT[class].load(Ordering::Relaxed);
        let alloc_bytes = ALLOC_BYTES[class].load(Ordering::Relaxed);
        let block_size = class_to_size(class);
        BinSnapshot {
            alloc_count,
            dealloc_count,
            alloc_bytes,
            block_size,
            live_estimate: alloc_count.saturating_sub(dealloc_count),
        }
    })
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

/// Process-wide live bytes in the small allocator:
/// sum of `live_estimate × block_size` across all classes.
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

/// Resets all per-class counters to zero.
///
/// Useful for marking the start of a profiling window so that subsequent
/// snapshots reflect only the activity since the reset.
#[inline]
pub fn reset_bin_stats() {
    for class in 0..NUM_SIZE_CLASSES {
        ALLOC_COUNT[class].store(0, Ordering::Relaxed);
        DEALLOC_COUNT[class].store(0, Ordering::Relaxed);
        ALLOC_BYTES[class].store(0, Ordering::Relaxed);
    }
}

/// One-line human-readable summary of process-wide bin stats.
///
/// Example: `"allocs=1234 live_bytes=65536 hottest_class=5(48b)"`.
/// Intended for diagnostic logging; format may change between versions.
#[must_use]
pub fn summary_line() -> std::string::String {
    let total_allocs = total_alloc_count();
    let live = total_live_bytes();
    match hottest_class() {
        Some(cls) => {
            let block = class_to_size(cls);
            std::format!("allocs={total_allocs} live_bytes={live} hottest_class={cls}({block}b)")
        }
        None => std::format!("allocs={total_allocs} live_bytes={live} hottest_class=none"),
    }
}
