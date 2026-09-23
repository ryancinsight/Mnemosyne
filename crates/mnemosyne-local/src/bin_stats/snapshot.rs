/// Per-size-class allocation statistics snapshot.
///
/// Non-exhaustive: this is telemetry the allocator *produces*, and its field
/// set grows as new counters are added — `requested_bytes` was the most recent.
/// Marking it so keeps each addition a non-breaking change instead of a major
/// one. Construct via [`Default`] and read the fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
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
    /// Counters advanced since `baseline`, saturating at zero where one
    /// decreased or was reset.
    ///
    /// Lives here rather than at the call site because [`BinSnapshot`] is
    /// `#[non_exhaustive]`: only this crate may build one by literal, so a new
    /// counter field extends this method instead of breaking every consumer
    /// that computes a delta.
    #[must_use]
    pub fn saturating_delta(&self, baseline: &Self) -> Self {
        Self {
            alloc_count: self.alloc_count.saturating_sub(baseline.alloc_count),
            dealloc_count: self.dealloc_count.saturating_sub(baseline.dealloc_count),
            alloc_bytes: self.alloc_bytes.saturating_sub(baseline.alloc_bytes),
            requested_bytes: self
                .requested_bytes
                .saturating_sub(baseline.requested_bytes),
            block_size: self.block_size,
            live_estimate: self.live_estimate.saturating_sub(baseline.live_estimate),
        }
    }

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
