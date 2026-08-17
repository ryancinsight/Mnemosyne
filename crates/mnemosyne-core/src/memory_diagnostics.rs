//! Memory allocation diagnostics and metrics tracking.
//!
//! This module provides comprehensive diagnostics for allocation patterns,
//! fragmentation analysis, and cache efficiency metrics. It tracks allocation
//! size distribution, per-size-class fragmentation, and fast-path cache hit/miss
//! ratios to help identify optimization opportunities.

use crate::constants::NUM_SIZE_CLASSES;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Tracks allocation metrics for a specific size class.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SizeClassMetrics {
    /// Number of allocations requested for this size class.
    pub allocation_count: usize,
    /// Total bytes allocated (cumulative, not current).
    pub total_bytes_allocated: usize,
    /// Number of deallocations for this size class.
    pub deallocation_count: usize,
    /// Total bytes deallocated.
    pub total_bytes_deallocated: usize,
    /// Number of blocks lost to fragmentation in this size class.
    pub fragmented_blocks: usize,
    /// Average utilization of pages in this size class (0-100%).
    pub page_utilization_percent: u8,
    /// Number of cache hits in fast-path allocations for this size class.
    pub cache_hits: usize,
    /// Number of cache misses requiring slower allocation paths.
    pub cache_misses: usize,
}

impl SizeClassMetrics {
    /// All-zero metrics; usable in const contexts (the derived Default is not const).
    pub const fn zeroed() -> Self {
        Self {
            allocation_count: 0,
            total_bytes_allocated: 0,
            deallocation_count: 0,
            total_bytes_deallocated: 0,
            fragmented_blocks: 0,
            page_utilization_percent: 0,
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    /// Returns the current allocation count minus deallocation count (live allocations).
    #[inline]
    pub fn live_allocations(&self) -> usize {
        self.allocation_count
            .saturating_sub(self.deallocation_count)
    }

    /// Returns the cache hit ratio as a percentage (0-100).
    #[inline]
    pub fn cache_hit_ratio(&self) -> u8 {
        let total = self.cache_hits.saturating_add(self.cache_misses);
        if total == 0 {
            0
        } else {
            ((self.cache_hits as u128 * 100) / total as u128) as u8
        }
    }

    /// Returns fragmentation ratio as a percentage.
    /// Higher values indicate more wasted space due to fragmentation.
    #[inline]
    pub fn fragmentation_ratio(&self) -> u8 {
        let allocated = self.total_bytes_allocated;
        let wasted = self.total_bytes_deallocated;
        if allocated == 0 {
            0
        } else {
            ((wasted as u128 * 100) / allocated as u128) as u8
        }
    }
}

/// Global allocation diagnostics accumulator.
///
/// This structure is designed to be updated by atomic operations from
/// multiple threads without locks. Each thread-local allocator maintains
/// its own copy and lazily folds into this global state.
pub struct AllocationDiagnostics {
    /// Per-size-class metrics array.
    metrics: [SizeClassMetrics; NUM_SIZE_CLASSES],
    /// Total allocations across all size classes (updated atomically).
    total_allocations: AtomicUsize,
    /// Total deallocations across all size classes (updated atomically).
    total_deallocations: AtomicUsize,
    /// Total bytes currently allocated (updated atomically).
    total_current_bytes: AtomicUsize,
    /// Peak bytes allocated (high-water mark).
    peak_bytes: AtomicUsize,
    /// Total cache hits across all size classes (updated atomically).
    total_cache_hits: AtomicUsize,
    /// Total cache misses across all size classes (updated atomically).
    total_cache_misses: AtomicUsize,
}

impl Default for AllocationDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}

impl AllocationDiagnostics {
    /// Creates a new, zero-initialized allocation diagnostics tracker.
    pub const fn new() -> Self {
        Self {
            metrics: [SizeClassMetrics::zeroed(); NUM_SIZE_CLASSES],
            total_allocations: AtomicUsize::new(0),
            total_deallocations: AtomicUsize::new(0),
            total_current_bytes: AtomicUsize::new(0),
            peak_bytes: AtomicUsize::new(0),
            total_cache_hits: AtomicUsize::new(0),
            total_cache_misses: AtomicUsize::new(0),
        }
    }

    /// Records an allocation of the given size in the specified size class.
    #[inline]
    pub fn record_allocation(&self, size_class: usize, size: usize) {
        if size_class < NUM_SIZE_CLASSES {
            self.total_allocations.fetch_add(1, Ordering::Relaxed);
            self.total_current_bytes.fetch_add(size, Ordering::Relaxed);

            // Update peak bytes if current exceeds peak
            let mut current = self.total_current_bytes.load(Ordering::Relaxed);
            let mut peak = self.peak_bytes.load(Ordering::Relaxed);
            while current > peak {
                match self.peak_bytes.compare_exchange_weak(
                    peak,
                    current,
                    Ordering::Release,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(p) => peak = p,
                }
                current = self.total_current_bytes.load(Ordering::Relaxed);
            }
        }
    }

    /// Records a deallocation of the given size in the specified size class.
    #[inline]
    pub fn record_deallocation(&self, size_class: usize, size: usize) {
        if size_class < NUM_SIZE_CLASSES {
            self.total_deallocations.fetch_add(1, Ordering::Relaxed);
            self.total_current_bytes.fetch_sub(
                size.min(self.total_current_bytes.load(Ordering::Relaxed)),
                Ordering::Relaxed,
            );
        }
    }

    /// Records a cache hit for fast-path allocation in the specified size class.
    #[inline]
    pub fn record_cache_hit(&self, size_class: usize) {
        if size_class < NUM_SIZE_CLASSES {
            self.total_cache_hits.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Records a cache miss (slower allocation path) in the specified size class.
    #[inline]
    pub fn record_cache_miss(&self, size_class: usize) {
        if size_class < NUM_SIZE_CLASSES {
            self.total_cache_misses.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Returns a snapshot of metrics for a specific size class.
    #[inline]
    pub fn size_class_metrics(&self, size_class: usize) -> Option<SizeClassMetrics> {
        if size_class < NUM_SIZE_CLASSES {
            Some(self.metrics[size_class])
        } else {
            None
        }
    }

    /// Returns the total number of allocations (cumulative).
    #[inline]
    pub fn total_allocations(&self) -> usize {
        self.total_allocations.load(Ordering::Relaxed)
    }

    /// Returns the total number of deallocations (cumulative).
    #[inline]
    pub fn total_deallocations(&self) -> usize {
        self.total_deallocations.load(Ordering::Relaxed)
    }

    /// Returns current bytes allocated.
    #[inline]
    pub fn current_bytes(&self) -> usize {
        self.total_current_bytes.load(Ordering::Relaxed)
    }

    /// Returns peak bytes allocated (high-water mark).
    #[inline]
    pub fn peak_bytes(&self) -> usize {
        self.peak_bytes.load(Ordering::Relaxed)
    }

    /// Returns total cache hits.
    #[inline]
    pub fn total_cache_hits(&self) -> usize {
        self.total_cache_hits.load(Ordering::Relaxed)
    }

    /// Returns total cache misses.
    #[inline]
    pub fn total_cache_misses(&self) -> usize {
        self.total_cache_misses.load(Ordering::Relaxed)
    }

    /// Returns overall cache hit ratio as a percentage.
    #[inline]
    pub fn overall_cache_hit_ratio(&self) -> u8 {
        let hits = self.total_cache_hits.load(Ordering::Relaxed);
        let misses = self.total_cache_misses.load(Ordering::Relaxed);
        let total = hits.saturating_add(misses);
        if total == 0 {
            0
        } else {
            ((hits as u128 * 100) / total as u128) as u8
        }
    }

    /// Returns live allocations (allocations - deallocations).
    #[inline]
    pub fn live_allocations(&self) -> usize {
        self.total_allocations
            .load(Ordering::Relaxed)
            .saturating_sub(self.total_deallocations.load(Ordering::Relaxed))
    }
}

/// Snapshot of memory efficiency metrics for diagnostics and reporting.
#[derive(Clone, Debug)]
pub struct MemoryEfficiencyReport {
    /// Per-size-class metrics.
    pub size_class_metrics: alloc::vec::Vec<SizeClassMetrics>,
    /// Total allocations recorded.
    pub total_allocations: usize,
    /// Total deallocations recorded.
    pub total_deallocations: usize,
    /// Currently allocated bytes.
    pub current_allocated_bytes: usize,
    /// Peak allocated bytes (high-water mark).
    pub peak_allocated_bytes: usize,
    /// Overall cache hit ratio (0-100%).
    pub cache_hit_ratio: u8,
    /// Average page utilization across all size classes.
    pub avg_page_utilization: u8,
    /// Estimated fragmentation overhead as a percentage.
    pub fragmentation_overhead: u8,
}

impl MemoryEfficiencyReport {
    /// Generates a memory efficiency report from the current diagnostics state.
    pub fn from_diagnostics(diags: &AllocationDiagnostics) -> Self {
        let mut metrics_vec = alloc::vec::Vec::with_capacity(NUM_SIZE_CLASSES);
        let mut total_utilization = 0u32;

        for i in 0..NUM_SIZE_CLASSES {
            if let Some(metrics) = diags.size_class_metrics(i) {
                total_utilization += metrics.page_utilization_percent as u32;
                metrics_vec.push(metrics);
            }
        }

        let avg_page_utilization = if NUM_SIZE_CLASSES > 0 {
            (total_utilization / NUM_SIZE_CLASSES as u32) as u8
        } else {
            0
        };

        let total_allocated = diags.total_allocations();
        let total_deallocated = diags.total_deallocations();
        let fragmentation_overhead = if total_allocated > 0 {
            ((total_deallocated as u128 * 100) / total_allocated as u128) as u8
        } else {
            0
        };

        Self {
            size_class_metrics: metrics_vec,
            total_allocations: total_allocated,
            total_deallocations: total_deallocated,
            current_allocated_bytes: diags.current_bytes(),
            peak_allocated_bytes: diags.peak_bytes(),
            cache_hit_ratio: diags.overall_cache_hit_ratio(),
            avg_page_utilization,
            fragmentation_overhead,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_class_metrics_live_allocations() {
        let mut metrics = SizeClassMetrics::default();
        metrics.allocation_count = 100;
        metrics.deallocation_count = 30;
        assert_eq!(metrics.live_allocations(), 70);
    }

    #[test]
    fn test_cache_hit_ratio() {
        let mut metrics = SizeClassMetrics::default();
        metrics.cache_hits = 80;
        metrics.cache_misses = 20;
        assert_eq!(metrics.cache_hit_ratio(), 80);
    }

    #[test]
    fn test_diagnostics_atomic_operations() {
        let diags = AllocationDiagnostics::new();
        diags.record_allocation(0, 16);
        diags.record_allocation(1, 32);
        assert_eq!(diags.total_allocations(), 2);
        assert_eq!(diags.current_bytes(), 48);
        assert_eq!(diags.peak_bytes(), 48);

        diags.record_deallocation(0, 16);
        assert_eq!(diags.total_deallocations(), 1);
        assert_eq!(diags.current_bytes(), 32);
    }

    #[test]
    fn test_cache_hit_recording() {
        let diags = AllocationDiagnostics::new();
        diags.record_cache_hit(0);
        diags.record_cache_hit(0);
        diags.record_cache_miss(0);
        assert_eq!(diags.total_cache_hits(), 2);
        assert_eq!(diags.total_cache_misses(), 1);
        assert_eq!(diags.overall_cache_hit_ratio(), 66);
    }
}
