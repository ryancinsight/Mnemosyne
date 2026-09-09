//! Human- and machine-readable summaries over the bin statistics.

/// Returns a human-readable one-line summary of the current policy and telemetry.
///
/// Format: `policy=<name> mitigations=0x<flags> allocs=<n> live_bytes=<b> int_frag=<x>%`
#[must_use]
pub fn policy_summary() -> alloc::string::String {
    use alloc::format;
    use mnemosyne_core::policy::AllocPolicy;
    let stats = mnemosyne_local::summary_line();
    format!(
        "policy={} mitigations=0x{:08X} fingerprint=0x{:016X} {stats}",
        mnemosyne_core::policy::StandardPolicy::POLICY_NAME,
        mnemosyne_core::policy::StandardPolicy::MITIGATION_FLAGS,
        mnemosyne_core::policy::StandardPolicy::POLICY_FINGERPRINT,
    )
}

/// Returns the `n` hottest size classes by alloc_count, sorted descending.
///
/// Flushes TLS stats before sampling. Returns at most `n` entries; fewer
/// if fewer than `n` classes have been allocated from.
#[must_use]
pub fn top_n_classes(n: usize) -> alloc::vec::Vec<mnemosyne_local::BinSnapshot> {
    let mut snapshots: alloc::vec::Vec<_> = mnemosyne_local::all_bin_snapshots()
        .into_iter()
        .filter(|s| s.alloc_count > 0)
        .collect();
    snapshots.sort_unstable_by_key(|s| core::cmp::Reverse(s.alloc_count));
    snapshots.truncate(n);
    snapshots
}

// ── Stats window ──────────────────────────────────────────────────────────────

/// A snapshot of bin stats taken at a fixed point in time.
///
/// Create a baseline with [`BinStatsWindow::capture`], then call
/// [`BinStatsWindow::delta`] later to compute per-class deltas over the window.
/// This is the recommended pattern for profiling a code region:
///
/// ```rust
/// # use mnemosyne::BinStatsWindow;
/// let baseline = BinStatsWindow::capture();
/// // ... code under profiling ...
/// let delta = baseline.delta();
/// ```
pub struct BinStatsWindow {
    bins: [mnemosyne_local::BinSnapshot; mnemosyne_core::NUM_SIZE_CLASSES],
}

impl BinStatsWindow {
    /// Captures the current per-class bin stats as a baseline.
    ///
    /// Flushes the calling thread's TLS batch first so the snapshot
    /// reflects all preceding allocations on this thread.
    #[must_use]
    pub fn capture() -> Self {
        Self {
            bins: mnemosyne_local::all_bin_snapshots(),
        }
    }

    /// Computes per-class deltas since the baseline was captured.
    ///
    /// Each returned snapshot has its counters set to the difference since
    /// the baseline. Counters that decreased (or were reset) saturate to zero.
    #[must_use]
    pub fn delta(&self) -> [mnemosyne_local::BinSnapshot; mnemosyne_core::NUM_SIZE_CLASSES] {
        let now = mnemosyne_local::all_bin_snapshots();
        core::array::from_fn(|class| {
            let b = &self.bins[class];
            let n = &now[class];
            n.saturating_delta(b)
        })
    }

    /// Total allocations during the window across all size classes.
    #[must_use]
    pub fn total_alloc_count_delta(&self) -> u64 {
        self.delta()
            .iter()
            .map(|s| s.alloc_count)
            .fold(0u64, u64::saturating_add)
    }

    /// Total live bytes at the end of the window minus the start.
    #[must_use]
    pub fn total_live_bytes_delta(&self) -> u64 {
        self.delta()
            .iter()
            .map(|s| s.live_bytes())
            .fold(0u64, u64::saturating_add)
    }

    /// Total user-requested bytes during the window across all size classes.
    ///
    /// Requires `record_alloc_with_size` to have been used at call sites.
    #[must_use]
    pub fn total_requested_bytes_delta(&self) -> u64 {
        self.delta()
            .iter()
            .map(|s| s.requested_bytes)
            .fold(0u64, u64::saturating_add)
    }

    /// Internal fragmentation ratio over the window:
    /// `(total_alloc_bytes_delta - total_requested_bytes_delta) / total_alloc_bytes_delta`.
    ///
    /// Returns `0.0` when `total_alloc_bytes_delta == 0` or `requested` is zero.
    #[must_use]
    pub fn window_internal_fragmentation(&self) -> f64 {
        let d = self.delta();
        let alloc: u64 = d.iter().map(|s| s.alloc_bytes).fold(0, u64::saturating_add);
        let req: u64 = d
            .iter()
            .map(|s| s.requested_bytes)
            .fold(0, u64::saturating_add);
        if alloc == 0 || req == 0 {
            return 0.0;
        }
        let waste = alloc.saturating_sub(req);
        (waste as f64 / alloc as f64).min(1.0)
    }
}
