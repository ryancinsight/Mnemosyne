//! Per-size-class allocation telemetry.
//!
//! Tracks process-wide allocation and deallocation counts per size class using
//! relaxed atomic counters. The allocation-byte total is derived from the
//! immutable class stride. Per-thread counters batch global updates so the
//! allocator hot path does not contend on one cache line for every operation.
//!
//! The counters are always enabled (no feature gate). They are not
//! synchronised with each other — a snapshot can observe counts from
//! different points in time — and pending per-thread batches may not yet be
//! visible. Each global counter is monotone-non-decreasing, so the
//! live-allocation estimate `alloc_count - dealloc_count` is an under-estimate
//! until the owning thread flushes its pending batch rather than a negative
//! artefact.
//!
//! ## Reset boundary (generation counter)
//!
//! `reset_bin_stats()` increments a process-wide `RESET_GENERATION` counter.
//! Each TLS batch records the generation it was started in. When a batch flush
//! sees that the global generation has advanced past its recorded generation,
//! it silently discards the batch instead of adding stale observations to the
//! fresh counters. This ensures that every worker's pre-reset activity is
//! excluded from post-reset snapshots regardless of flush ordering.
//!
//! Fragmentation ratio per class: `(alloc_bytes - dealloc_bytes) /
//! alloc_bytes`. Internal fragmentation per class: `(alloc_bytes -
//! requested_bytes) / alloc_bytes`.

use core::sync::atomic::{AtomicU64, Ordering};
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use mnemosyne_core::size_class::class_to_size;

mod batch;
mod snapshot;

pub(crate) use batch::{record_alloc_with_size, record_dealloc};
pub use snapshot::BinSnapshot;

/// Flushes the TLS batch then sums every entry of a global counter array.
///
/// Both [`total_alloc_count`] and [`total_requested_bytes`] share this body —
/// the only difference between them is which array is summed.
#[inline]
fn sum_counter(arr: &[AtomicU64; NUM_SIZE_CLASSES]) -> u64 {
    batch::flush_current_thread();
    arr.iter()
        .map(|c| c.load(Ordering::Relaxed))
        .fold(0u64, u64::saturating_add)
}

/// Constructs a [`BinSnapshot`] for a single size class.
///
/// Non-generic SSOT shared by [`bin_snapshot`] and [`all_bin_snapshots`],
/// eliminating the 5-line construction that previously appeared in both.
/// Callers must flush the per-thread batch counters first.
#[inline(always)]
fn make_bin_snapshot(class: usize) -> BinSnapshot {
    let alloc_count = batch::ALLOC_COUNT[class].load(Ordering::Relaxed);
    let dealloc_count = batch::DEALLOC_COUNT[class].load(Ordering::Relaxed);
    let block_size = class_to_size(class);
    BinSnapshot {
        alloc_count,
        dealloc_count,
        alloc_bytes: batch::allocation_bytes(alloc_count, block_size),
        requested_bytes: batch::REQUESTED_BYTES[class].load(Ordering::Relaxed),
        block_size,
        live_estimate: alloc_count.saturating_sub(dealloc_count),
    }
}

/// Returns a snapshot for size class `class`, or `None` if out of range.
#[must_use]
pub fn bin_snapshot(class: usize) -> Option<BinSnapshot> {
    if class >= NUM_SIZE_CLASSES {
        return None;
    }
    batch::flush_current_thread();
    Some(make_bin_snapshot(class))
}

/// Returns snapshots for all `NUM_SIZE_CLASSES` size classes.
#[must_use]
pub fn all_bin_snapshots() -> [BinSnapshot; NUM_SIZE_CLASSES] {
    batch::flush_current_thread();
    core::array::from_fn(make_bin_snapshot)
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
    sum_counter(&batch::ALLOC_COUNT)
}

/// Resets all per-class counters to zero.
///
/// Useful for marking the start of a profiling window so subsequent
/// snapshots reflect only activity since the reset.
pub fn reset_bin_stats() {
    // Advance the reset generation BEFORE zeroing, so any concurrent
    // flush that reads the new generation discards its batch rather than
    // flushing stale pre-reset counts that would be immediately zeroed.
    //
    // Relaxed is the whole requirement: the flush guard compares this one
    // variable against its own stamped value, and single-variable
    // modification-order coherence -- which Relaxed already guarantees --
    // is what makes that comparison monotonic. No happens-before edge with
    // the counters is needed, because every counter access is Relaxed and a
    // flush that still reads the old generation only adds counts the
    // zeroing below immediately erases. A stronger ordering here would not
    // change what the Relaxed loads at the guard can observe.
    batch::RESET_GENERATION.fetch_add(1, Ordering::Relaxed);
    batch::flush_current_thread();
    for class in 0..NUM_SIZE_CLASSES {
        batch::ALLOC_COUNT[class].store(0, Ordering::Relaxed);
        batch::DEALLOC_COUNT[class].store(0, Ordering::Relaxed);
        batch::REQUESTED_BYTES[class].store(0, Ordering::Relaxed);
    }
}

/// Flushes the calling thread's pending bin-stats batch to the global counters.
///
/// `bin_snapshot` and `all_bin_snapshots` call this automatically; invoke
/// it explicitly before reading from a different thread.
#[inline]
pub fn flush_tls_stats() {
    batch::flush_current_thread();
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
    sum_counter(&batch::REQUESTED_BYTES)
}

/// Process-wide internal fragmentation ratio:
/// `(total_alloc_bytes - total_requested_bytes) / total_alloc_bytes`.
///
/// Returns `0.0` when `total_alloc_bytes == 0` or `total_requested_bytes == 0`
/// (e.g. before any `record_alloc_with_size` calls).
#[must_use]
pub fn total_internal_fragmentation() -> f64 {
    let snapshots = all_bin_snapshots();
    let alloc: u64 = snapshots
        .iter()
        .map(|s| s.alloc_bytes)
        .fold(0, u64::saturating_add);
    let requested: u64 = snapshots
        .iter()
        .map(|s| s.requested_bytes)
        .fold(0, u64::saturating_add);
    if alloc == 0 || requested == 0 {
        return 0.0;
    }
    let waste = alloc.saturating_sub(requested);
    (waste as f64 / alloc as f64).min(1.0)
}

/// Returns the current value of the reset generation counter.
///
/// Increments monotonically on every [`reset_bin_stats`] call. Useful
/// for asserting that a snapshot spans exactly one reset-free window.
#[inline]
#[must_use]
pub fn reset_generation_count() -> u32 {
    batch::RESET_GENERATION.load(Ordering::Relaxed)
}

/// Returns the fractional distribution of `alloc_count` across all size
/// classes as an array of `f64` values in `[0.0, 1.0]` that sum to 1.0.
///
/// The `i`-th element is `alloc_count[i] / total_alloc_count`. If no
/// allocations have been recorded, every element is `0.0`.
///
/// Useful for understanding which size classes dominate the workload.
#[must_use]
pub fn alloc_distribution() -> [f64; NUM_SIZE_CLASSES] {
    let snapshots = all_bin_snapshots();
    let total: u64 = snapshots
        .iter()
        .map(|s| s.alloc_count)
        .fold(0, u64::saturating_add);
    if total == 0 {
        return [0.0; NUM_SIZE_CLASSES];
    }
    let mut dist = [0.0f64; NUM_SIZE_CLASSES];
    for (i, s) in snapshots.iter().enumerate() {
        dist[i] = s.alloc_count as f64 / total as f64;
    }
    dist
}

#[cfg(test)]
mod tests {
    use core::sync::atomic::AtomicU64;
    use std::sync::{Arc, Barrier};

    use super::batch::{FLUSH_BATCH, PendingCount, RESET_GENERATION, record_alloc_with_size};
    use super::{
        NUM_SIZE_CLASSES, all_bin_snapshots, bin_snapshot, class_to_size, flush_tls_stats,
        reset_bin_stats, reset_generation_count,
    };

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
    fn generation_counter_discards_stale_batches() {
        // Build a pending slot with the CURRENT generation.
        let global = [const { AtomicU64::new(0) }; NUM_SIZE_CLASSES];
        let mut pending = PendingCount::new();
        // Prime the slot so `generation` is stamped.
        pending.record(2, &global);
        // Advance the global generation (simulates a reset_bin_stats call).
        RESET_GENERATION.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // Flush: the batch is stale and must be discarded.
        pending.flush(&global);
        // The global array must not have received the stale count.
        assert_eq!(
            global[2].load(core::sync::atomic::Ordering::Relaxed),
            0,
            "stale batch must not flush after generation advance"
        );
        // Undo the generation increment to avoid interfering with other tests.
        RESET_GENERATION.fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
    }

    /// Multi-threaded boundary proof for MN-BIN-STATS-RESET-BOUNDARY.
    ///
    /// Verifies that `reset_bin_stats` advances the generation counter, which
    /// is the mechanism that makes it a true profiling boundary: TLS batches
    /// stamped with an older generation are discarded on flush rather than
    /// added to the fresh post-reset counters. The single-thread property is
    /// proven by `generation_counter_discards_stale_batches`; this test
    /// confirms the public API increments the counter monotonically so the
    /// generation-based discard is actually triggered.
    #[test]
    fn reset_bin_stats_monotonically_advances_generation() {
        let gen_before = reset_generation_count();
        reset_bin_stats();
        let gen_after = reset_generation_count();
        assert!(
            gen_after > gen_before,
            "reset_bin_stats must advance the generation counter: \
             before={gen_before} after={gen_after}"
        );
        // Post-reset: all class counters must be zero (this thread has no live
        // pending batch since `reset_bin_stats` also flushes the calling thread).
        for (class, snap) in all_bin_snapshots().iter().enumerate() {
            assert_eq!(
                snap.alloc_count, 0,
                "class {class} alloc_count must be zero immediately after reset"
            );
            assert_eq!(
                snap.dealloc_count, 0,
                "class {class} dealloc_count must be zero immediately after reset"
            );
        }
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

    /// Cross-thread boundary proof for MN-BIN-STATS-RESET-BOUNDARY: a batch
    /// accumulated on another thread before `reset_bin_stats` runs must not
    /// leak into the post-reset counters once that thread finally flushes.
    #[test]
    fn reset_excludes_a_batch_pending_on_another_thread() {
        const CLASS: usize = 5;
        // Both counts stay below FLUSH_BATCH, so neither batch flushes on its
        // own and only the explicit flushes move the global counter.
        const PRE_RESET: u32 = FLUSH_BATCH / 2;
        const POST_RESET: u32 = FLUSH_BATCH / 4;

        let barrier = Arc::new(Barrier::new(2));
        let worker = {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                for _ in 0..PRE_RESET {
                    record_alloc_with_size(CLASS, class_to_size(CLASS));
                }
                barrier.wait(); // pre-reset batch is pending
                barrier.wait(); // reset has completed
                flush_tls_stats();
                for _ in 0..POST_RESET {
                    record_alloc_with_size(CLASS, class_to_size(CLASS));
                }
                flush_tls_stats();
            })
        };

        barrier.wait();
        reset_bin_stats();
        barrier.wait();
        worker.join().expect("worker thread panicked");

        let snapshot = bin_snapshot(CLASS).expect("invariant: CLASS < NUM_SIZE_CLASSES");
        assert_eq!(
            snapshot.alloc_count,
            u64::from(POST_RESET),
            "post-reset total must exclude the {PRE_RESET} allocations pending on the worker"
        );
        assert_eq!(
            snapshot.requested_bytes,
            u64::from(POST_RESET) * class_to_size(CLASS) as u64,
        );
    }
}
