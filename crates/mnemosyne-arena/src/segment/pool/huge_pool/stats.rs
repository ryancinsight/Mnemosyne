//! Advisory telemetry for the huge pool: retained block and byte counts and
//! the [`HugePoolStats`] snapshot. All reads are per-node `Relaxed` loads and
//! are not jointly consistent.

use super::super::HugePoolStats;
use super::GlobalHugePool;

impl GlobalHugePool {
    /// Advisory number of huge blocks currently retained across all NUMA
    /// nodes (`Relaxed` per-node loads; callers tolerate a small skew under
    /// concurrency, matching the count discipline of the tagged stacks).
    #[inline]
    pub fn retained_blocks(&self) -> usize {
        self.nodes
            .iter()
            .map(|node| {
                node.total_count
                    .value
                    .load(core::sync::atomic::Ordering::Relaxed)
            })
            .sum()
    }

    /// Advisory total bytes of huge blocks currently retained across all NUMA
    /// nodes (`Relaxed` per-node loads, same skew tolerance as
    /// [`Self::retained_blocks`]).
    #[inline]
    pub fn retained_bytes(&self) -> usize {
        self.nodes
            .iter()
            .map(|node| {
                node.total_bytes
                    .value
                    .load(core::sync::atomic::Ordering::Relaxed)
            })
            .sum()
    }

    /// Returns a point-in-time snapshot of the huge pool's key counters.
    ///
    /// All fields are individually relaxed reads — not jointly consistent.
    #[inline]
    #[must_use]
    pub fn stats(&self) -> HugePoolStats {
        HugePoolStats {
            retained_blocks: self.retained_blocks(),
            retained_bytes: self.retained_bytes(),
        }
    }
}
