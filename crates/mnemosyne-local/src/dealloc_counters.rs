//! Per-branch atomic counter instrumentation for `thread_free`.
//!
//! This opt-in probe surfaces which `thread_free` commit arm serves a
//! deallocation workload. It only compiles when the `dealloc-probe`
//! Cargo feature is enabled, so default builds do not include the
//! module or call sites.
//!
//! # Surface
//!
//! [`DeallocPath`] enumerates the five commit points in
//! `thread_free_classified`:
//!
//! 1. [`DeallocPath::HugeClassifier`] — `page.block_size == 0` branch
//!    reached (rare on the `large_8192` row).
//! 2. [`DeallocPath::InPlaceSmall`] — same-owner active-page free
//!    that stays in the active list (page_alloc_count>1 or current
//!    segment).
//! 3. [`DeallocPath::ActiveFreeLastBlock`] — same-owner active-page
//!    free of the *last* live block (page_alloc_count==1 or non-current
//!    segment); the page may or may not actually transition to
//!    `empty_pages` because the `is_only_active` short-circuit leaves
//!    a sole active page in place. Counter records the arm the dealloc
//!    reached, not the page-list mutation (the mutation is a refcount
//!    detail tracked separately).
//! 4. [`DeallocPath::FullToActive`] — full page transitions back to
//!    the active list on first free.
//! 5. [`DeallocPath::ColdOrRecursing`] — `thread_free_cold` fallback
//!    for genuinely cross-thread frees, owner-mismatch frees, AND
//!    re-entrant frees where `is_allocating == true` made the inner
//!    arm fall through without recording a hot-path label. This is
//!    a coarse bucket by design (splitting recursing out is a Phase
//!    5 candidate) but it is the *single* bucket every fall-through
//!    path lands on, so the partition is sound.
//!
//! # Caveat for `--enforce-thresholds` runs
//!
//! Any `benchmark_summary --enforce-thresholds` invocation that gates
//! an allocator change must run on the default-feature build, not on
//! `--features dealloc-probe`. The probe intentionally adds one
//! Relaxed atomic increment per deallocation commit, so use it only
//! when the goal is to read the branch-mix snapshot.
//!
//! # Layout
//!
//! Five `AtomicU64` counters live in a single `[AtomicU64; 5]` static.
//! [`record`] does a Relaxed `fetch_add(1)`, so the bench hot-path
//! pays one Relaxed atomic increment per dealloc. The probe is only
//! wired at *commit* points (just
//! before a `return` or fall-through), so the recorded count reflects
//! the branch the dealloc actually took, not the path the dealloc
//! entered but later bounced out of via a corruption guard.
//!
//! [`reset`] is **not** synchronized against concurrent `record()`
//! calls (uses `Ordering::Relaxed` per counter). Callers that need a
//! scoped measurement window MUST ensure all prior deallocations have
//! retired before calling `reset()` — in the single-threaded bench
//! A/B scenario this is satisfied implicitly.
//!
//! # Read
//!
//! [`snapshot`] returns a fixed-size `[(DeallocPath, &'static str, u64); 5]`
//! suitable for `println!` or log capture. [`reset`] zeroes the
//! counters so a fresh measurement window can start; pair with a
//! test or post-bench hook to surface the data without committing to
//! a particular harness plumbing in the hot path.

use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum DeallocPath {
    /// `page.block_size == 0` branch reached (large/huge unwrap).
    /// Rare on small/free-list `large_8192` rows.
    HugeClassifier = 0,
    /// In-place active free: `page_alloc_count > 1` or
    /// `alloc.is_current_segment(segment)`. This is the dominant
    /// path for fresh-one-block `Page` rows under the same owner.
    InPlaceSmall = 1,
    /// Same-owner active page free whose `page_alloc_count` was
    /// about to drop to zero (i.e. the *last* live block of the page
    /// is being returned). The page may or may not actually leave
    /// the active list — the `is_only_active` short-circuit keeps a
    /// sole active page in place — so the counter records *which arm
    /// the dealloc reached*, not whether the page-list mutation
    /// happened. Phase 5 candidates can split this label further if
    /// the page-list mutation cost becomes the focus.
    ActiveFreeLastBlock = 2,
    /// Full page transitions back to active (first free from a full
    /// page; the existing branded full→active list token op).
    FullToActive = 3,
    /// `thread_free_cold` fallback — genuinely cross-thread frees,
    /// owner-mismatch frees, AND re-entrant frees where
    /// `is_allocating == true` made an inner hot-path arm fall
    /// through without recording. Coarse bucket by design; the
    /// partition is sound because every fall-through path lands here.
    ColdOrRecursing = 4,
}

impl DeallocPath {
    /// Number of branch counters exposed by the probe.
    pub const COUNT: usize = 5;

    /// All five paths in stable order. Iteration order matches
    /// the rows returned by [`snapshot`].
    pub const ALL: [Self; Self::COUNT] = [
        Self::HugeClassifier,
        Self::InPlaceSmall,
        Self::ActiveFreeLastBlock,
        Self::FullToActive,
        Self::ColdOrRecursing,
    ];

    /// Stable index into the counter slice. `0..=4`.
    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Stable human-readable label for logs/snapshots.
    pub const fn name(self) -> &'static str {
        match self {
            Self::HugeClassifier => "huge_classifier",
            Self::InPlaceSmall => "in_place_small",
            Self::ActiveFreeLastBlock => "active_free_last_block",
            Self::FullToActive => "full_to_active",
            Self::ColdOrRecursing => "cold_or_recursing",
        }
    }
}

static COUNTS: [AtomicU64; DeallocPath::COUNT] = [const { AtomicU64::new(0) }; DeallocPath::COUNT];

/// Records that `path` was committed by the calling deallocation.
///
/// Always-on when the `dealloc-probe` feature is enabled. The atomic
/// increment uses Relaxed ordering — the dealloc fast-path pays a
/// single uncontended atomic increment. Gate the call site with
/// `#[cfg(feature = "dealloc-probe")]` so production builds
/// eliminate both the call and the load/store entirely.
#[inline(always)]
pub fn record(path: DeallocPath) {
    COUNTS[path.index()].fetch_add(1, Ordering::Relaxed);
}

/// Returns `(path, name, count)` rows in the order
/// [`DeallocPath::ALL`] walks. Useful for snapshotting before/after
/// a benchmark run without committing to a particular output
/// harness.
pub fn snapshot() -> [(DeallocPath, &'static str, u64); DeallocPath::COUNT] {
    DeallocPath::ALL.map(|path| {
        (
            path,
            path.name(),
            COUNTS[path.index()].load(Ordering::Relaxed),
        )
    })
}

/// Zeroes every counter. Pair with [`snapshot`] to scope a
/// measurement window — call before the bench pass, call after.
pub fn reset() {
    for c in &COUNTS {
        c.store(0, Ordering::Relaxed);
    }
}

/// Sum across all five counters. Useful for asserting in tests
/// that `total == bundle_call_count`.
pub fn total() -> u64 {
    COUNTS.iter().map(|c| c.load(Ordering::Relaxed)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_path_has_a_distinct_stable_index() {
        let mut seen = [false; DeallocPath::COUNT];
        for path in DeallocPath::ALL {
            let index = path.index();
            assert!(
                index < DeallocPath::COUNT,
                "{path:?} index {index} exceeds counter array length"
            );
            assert!(!seen[index], "duplicate index for {path:?}");
            seen[index] = true;
        }
        assert_eq!(
            seen,
            [true; DeallocPath::COUNT],
            "every counter slot should be covered by exactly one path"
        );
    }

    #[test]
    fn reset_clears_every_counter() {
        record(DeallocPath::InPlaceSmall);
        record(DeallocPath::FullToActive);
        assert!(total() >= 2);
        reset();
        for (_, _, count) in snapshot() {
            assert_eq!(count, 0, "counter not zeroed after reset()");
        }
        assert_eq!(total(), 0);
    }

    #[test]
    fn record_increments_only_the_targeted_path() {
        reset();
        for _ in 0..7 {
            record(DeallocPath::InPlaceSmall);
        }
        record(DeallocPath::ColdOrRecursing);
        let snap = snapshot();
        for (path, _name, count) in snap {
            let expected = match path {
                DeallocPath::InPlaceSmall => 7,
                DeallocPath::ColdOrRecursing => 1,
                _ => 0,
            };
            assert_eq!(count, expected, "{path:?} counter mismatch");
        }
    }
}
