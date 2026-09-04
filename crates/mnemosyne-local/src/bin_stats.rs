//! Per-size-class allocation telemetry.
//!
//! Tracks process-wide allocation and deallocation counts per size class using
//! relaxed atomic counters. The allocation-byte total is derived from the
//! immutable class stride. Per-thread counters batch global updates so the
//! allocator hot path does not contend on one cache line for every operation.
//!
//! The counters are always enabled (no feature gate).  They are not
//! synchronised with each other — a snapshot can observe counts from
//! different points in time — and pending per-thread batches may not yet be
//! visible. Each global counter is monotone-non-decreasing, so the
//! live-allocation estimate `alloc_count - dealloc_count` is an under-estimate
//! until the owning thread flushes its pending batch rather than a negative
//! artefact.
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
/// Cumulative user-requested bytes per class; used to compute internal
/// fragmentation: `(alloc_bytes - requested_bytes) / alloc_bytes`.
///
/// Updated with a direct relaxed `fetch_add` (not batched) because the
/// request size varies per call and cannot be accumulated in the
/// fixed-class `PendingCount` slots. The hot-path overhead is one extra
/// `LOCK XADD` per allocation, which is dominated by the cache-line cost
/// of the alloc itself.
static REQUESTED_BYTES: [AtomicU64; NUM_SIZE_CLASSES] =
    [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];

// Eight direct-mapped entries cover the common case of one or a few active
// size classes while keeping the per-thread footprint bounded at 256 bytes.
// A class collision flushes the displaced entry; it never drops observations.
const PENDING_SLOTS: usize = 8;
const FLUSH_BATCH: u32 = 64;
const EMPTY_CLASS: usize = usize::MAX;

const _: () = assert!(PENDING_SLOTS.is_power_of_two());

#[derive(Clone, Copy)]
struct PendingCount {
    class: usize,
    count: u32,
}

impl PendingCount {
    const fn new() -> Self {
        Self {
            class: EMPTY_CLASS,
            count: 0,
        }
    }

    #[inline(always)]
    fn record(&mut self, class: usize, global: &[AtomicU64; NUM_SIZE_CLASSES]) {
        if self.class != class {
            self.flush(global);
            self.class = class;
        }

        self.count += 1;
        if self.count == FLUSH_BATCH {
            self.flush(global);
        }
    }

    #[inline]
    fn flush(&mut self, global: &[AtomicU64; NUM_SIZE_CLASSES]) {
        if self.count != 0 {
            global[self.class].fetch_add(self.count as u64, Ordering::Relaxed);
            self.count = 0;
        }
    }
}

struct ThreadBinStats {
    alloc: [PendingCount; PENDING_SLOTS],
    dealloc: [PendingCount; PENDING_SLOTS],
}

impl ThreadBinStats {
    const fn new() -> Self {
        Self {
            alloc: [PendingCount::new(); PENDING_SLOTS],
            dealloc: [PendingCount::new(); PENDING_SLOTS],
        }
    }

    #[inline(always)]
    fn record_alloc(&mut self, class: usize) {
        self.alloc[class & (PENDING_SLOTS - 1)].record(class, &ALLOC_COUNT);
    }

    #[inline(always)]
    fn record_dealloc(&mut self, class: usize) {
        self.dealloc[class & (PENDING_SLOTS - 1)].record(class, &DEALLOC_COUNT);
    }

    #[inline]
    fn flush(&mut self) {
        for pending in &mut self.alloc {
            pending.flush(&ALLOC_COUNT);
        }
        for pending in &mut self.dealloc {
            pending.flush(&DEALLOC_COUNT);
        }
    }
}

impl Drop for ThreadBinStats {
    fn drop(&mut self) {
        self.flush();
    }
}

std::thread_local! {
    static THREAD_STATS: core::cell::UnsafeCell<ThreadBinStats> =
        const { core::cell::UnsafeCell::new(ThreadBinStats::new()) };
}

#[inline]
fn allocation_bytes(alloc_count: u64, block_size: usize) -> u64 {
    alloc_count.saturating_mul(block_size as u64)
}

/// Records one allocation with the explicit adjusted request size.
///
/// Updates both the batched alloc-count and the direct requested-bytes
/// counter so per-class internal fragmentation can be measured.
#[inline(always)]
pub(crate) fn record_alloc_with_size(class: usize, adjusted_size: usize) {
    if class < NUM_SIZE_CLASSES {
        THREAD_STATS.with(|stats| {
            // SAFETY: `THREAD_STATS` is owned by the current thread.
            unsafe { (*stats.get()).record_alloc(class) };
        });
        REQUESTED_BYTES[class].fetch_add(adjusted_size as u64, Ordering::Relaxed);
    }
}

/// Records one deallocation into `class`.
#[inline(always)]
pub(crate) fn record_dealloc(class: usize) {
    if class < NUM_SIZE_CLASSES {
        THREAD_STATS.with(|stats| {
            // SAFETY: `THREAD_STATS` is owned by the current thread. The
            // closure cannot run concurrently for the same TLS value.
            unsafe { (*stats.get()).record_dealloc(class) };
        });
    }
}

#[inline]
fn flush_current_thread() {
    THREAD_STATS.with(|stats| {
        // SAFETY: `THREAD_STATS` is owned by the current thread. The closure
        // cannot run concurrently for the same TLS value.
        unsafe { (*stats.get()).flush() };
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
    ///
    /// The product saturates at `u64::MAX` rather than wrapping.
    pub alloc_bytes: u64,
    /// Cumulative user-requested bytes for this class.
    ///
    /// Populated by `record_alloc_with_size`; zero when the call sites only
    /// use `record_alloc`. Internal fragmentation =
    /// `(alloc_bytes - requested_bytes) / alloc_bytes`.
    pub requested_bytes: u64,
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
    /// Fragmentation ratio: `live_bytes / alloc_bytes`, in `[0.0, 1.0]`.
    ///
    /// Returns `0.0` when nothing has ever been allocated in this class.
    #[inline]
    #[must_use]
    pub fn fragmentation_ratio(&self) -> f64 {
        if self.alloc_bytes == 0 {
            return 0.0;
        }
        let live_bytes = self.live_estimate.saturating_mul(self.block_size as u64);
        (live_bytes as f64 / self.alloc_bytes as f64).min(1.0)
    }

    /// Internal fragmentation: `(alloc_bytes - requested_bytes) / alloc_bytes`.
    ///
    /// Returns `0.0` when `requested_bytes` is zero (not tracked) or
    /// `alloc_bytes` is zero.
    #[inline]
    #[must_use]
    pub fn internal_fragmentation_ratio(&self) -> f64 {
        if self.alloc_bytes == 0 || self.requested_bytes == 0 {
            return 0.0;
        }
        let waste = self.alloc_bytes.saturating_sub(self.requested_bytes);
        (waste as f64 / self.alloc_bytes as f64).min(1.0)
    }

    /// Live bytes in this class: `live_estimate × block_size`.
    #[inline]
    #[must_use]
    pub fn live_bytes(&self) -> u64 {
        self.live_estimate.saturating_mul(self.block_size as u64)
    }
}

/// Returns a snapshot for size class `class`, or `None` if out of range.
#[must_use]
pub fn bin_snapshot(class: usize) -> Option<BinSnapshot> {
    if class >= NUM_SIZE_CLASSES {
        return None;
    }
    flush_current_thread();
    let alloc_count = ALLOC_COUNT[class].load(Ordering::Relaxed);
    let dealloc_count = DEALLOC_COUNT[class].load(Ordering::Relaxed);
    let block_size = class_to_size(class);
    Some(BinSnapshot {
        alloc_count,
        dealloc_count,
        alloc_bytes: allocation_bytes(alloc_count, block_size),
        requested_bytes: REQUESTED_BYTES[class].load(Ordering::Relaxed),
        block_size,
        live_estimate: alloc_count.saturating_sub(dealloc_count),
    })
}

/// Returns snapshots for all `NUM_SIZE_CLASSES` size classes.
#[must_use]
pub fn all_bin_snapshots() -> [BinSnapshot; NUM_SIZE_CLASSES] {
    flush_current_thread();
    core::array::from_fn(|class| {
        let alloc_count = ALLOC_COUNT[class].load(Ordering::Relaxed);
        let dealloc_count = DEALLOC_COUNT[class].load(Ordering::Relaxed);
        let block_size = class_to_size(class);
        BinSnapshot {
            alloc_count,
            dealloc_count,
            alloc_bytes: allocation_bytes(alloc_count, block_size),
            requested_bytes: REQUESTED_BYTES[class].load(Ordering::Relaxed),
            block_size,
            live_estimate: alloc_count.saturating_sub(dealloc_count),
        }
    })
}

/// Returns the index of the hottest size class (highest alloc_count), or
/// `None` when nothing has ever been allocated.
#[must_use]
pub fn hottest_class() -> Option<usize> {
    let snapshots = all_bin_snapshots();
    snapshots
        .iter()
        .enumerate()
        .max_by_key(|(_, s)| s.alloc_count)
        .and_then(|(idx, s)| if s.alloc_count > 0 { Some(idx) } else { None })
}

/// Process-wide live bytes in the small allocator: sum of `live_estimate ×
/// block_size` across all classes.
#[must_use]
pub fn total_live_bytes() -> u64 {
    all_bin_snapshots()
        .iter()
        .map(|s| s.live_bytes())
        .fold(0u64, u64::saturating_add)
}

/// Process-wide total allocation count across all small size classes.
#[must_use]
pub fn total_alloc_count() -> u64 {
    flush_current_thread();
    ALLOC_COUNT
        .iter()
        .map(|c| c.load(Ordering::Relaxed))
        .fold(0u64, u64::saturating_add)
}

/// Resets all per-class counters to zero.
///
/// Useful for marking the start of a profiling window so subsequent
/// snapshots reflect only activity since the reset.
pub fn reset_bin_stats() {
    flush_current_thread();
    for class in 0..NUM_SIZE_CLASSES {
        ALLOC_COUNT[class].store(0, Ordering::Relaxed);
        DEALLOC_COUNT[class].store(0, Ordering::Relaxed);
        REQUESTED_BYTES[class].store(0, Ordering::Relaxed);
    }
}

/// Flushes the calling thread's pending bin-stats batch to the global counters.
///
/// `bin_snapshot` and `all_bin_snapshots` call this automatically; invoke
/// it explicitly before reading from a different thread.
#[inline]
pub fn flush_tls_stats() {
    flush_current_thread();
}

/// One-line human-readable summary of process-wide bin stats.
#[must_use]
pub fn summary_line() -> std::string::String {
    let total_allocs = total_alloc_count();
    let live = total_live_bytes();
    let int_frag = total_internal_fragmentation();
    match hottest_class() {
        Some(cls) => std::format!(
            "allocs={total_allocs} live_bytes={live} int_frag={int_frag:.1}% hottest_class={cls}({}b)",
            class_to_size(cls)
        ),
        None => std::format!(
            "allocs={total_allocs} live_bytes={live} int_frag={int_frag:.1}% hottest_class=none"
        ),
    }
}

/// Process-wide cumulative user-requested bytes across all small size classes.
///
/// Zero until `record_alloc_with_size` call sites are wired (done in Phase 19).
#[must_use]
pub fn total_requested_bytes() -> u64 {
    flush_current_thread();
    REQUESTED_BYTES
        .iter()
        .map(|c| c.load(Ordering::Relaxed))
        .fold(0u64, u64::saturating_add)
}

/// Process-wide internal fragmentation ratio:
/// `(total_alloc_bytes - total_requested_bytes) / total_alloc_bytes`.
///
/// Returns `0.0` when `total_alloc_bytes == 0` or `total_requested_bytes == 0`
/// (e.g. before any `record_alloc_with_size` calls).
#[must_use]
pub fn total_internal_fragmentation() -> f64 {
    let snapshots = all_bin_snapshots();
    let alloc: u64 = snapshots.iter().map(|s| s.alloc_bytes).fold(0, u64::saturating_add);
    let requested: u64 = snapshots.iter().map(|s| s.requested_bytes).fold(0, u64::saturating_add);
    if alloc == 0 || requested == 0 {
        return 0.0;
    }
    let waste = alloc.saturating_sub(requested);
    (waste as f64 / alloc as f64).min(1.0)
}

#[cfg(test)]
mod tests {
    use core::sync::atomic::AtomicU64;

    use super::{FLUSH_BATCH, NUM_SIZE_CLASSES, PendingCount, all_bin_snapshots, bin_snapshot};

    #[test]
    fn pending_counts_flush_without_dropping_observations() {
        let global = [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];
        let mut pending = PendingCount::new();

        for _ in 0..FLUSH_BATCH {
            pending.record(3, &global);
        }

        assert_eq!(
            global[3].load(core::sync::atomic::Ordering::Relaxed),
            FLUSH_BATCH as u64
        );
        assert_eq!(pending.count, 0);
    }

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
