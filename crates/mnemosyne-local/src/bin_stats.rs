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

/// Records one allocation from `class`.
///
/// `# Safety` is not required; the function bounds-checks `class` internally.
/// Called on the alloc hot path: one TLS counter update and one global relaxed
/// `fetch_add` per batch of matching operations.
#[inline(always)]
pub(crate) fn record_alloc(class: usize) {
    if class < NUM_SIZE_CLASSES {
        THREAD_STATS.with(|stats| {
            // SAFETY: `THREAD_STATS` is owned by the current thread. The
            // closure cannot run concurrently for the same TLS value.
            unsafe { (*stats.get()).record_alloc(class) };
        });
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
    flush_current_thread();
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
    flush_current_thread();
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
