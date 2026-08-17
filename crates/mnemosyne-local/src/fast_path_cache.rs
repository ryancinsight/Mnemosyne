//! Thread-local fast-path allocation caches for common size classes.
//!
//! This module implements per-thread fast-path caches that avoid contention on
//! the global allocator for frequently-used allocation sizes (16, 32, 64, 128, 256 bytes).
//! Each cache uses a simple bump-pointer pool with generation counters to enable
//! efficient reuse.

use mnemosyne_core::NUM_SIZE_CLASSES;

/// Configuration for fast-path caches.
pub struct FastPathCacheConfig {
    /// Whether fast-path caching is enabled.
    pub enabled: bool,
    /// Maximum blocks to retain in each size class cache.
    pub max_blocks_per_class: usize,
    /// Size classes to cache (by index).
    pub cached_size_classes: &'static [usize],
}

impl Default for FastPathCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_blocks_per_class: 256,
            // Common sizes: 16, 32, 64, 128, 256 bytes (size class indices 0, 1, 3, 7, 11)
            cached_size_classes: &[0, 1, 3, 7, 11],
        }
    }
}

/// A single slot in a fast-path cache for a specific size class.
#[derive(Clone, Copy, Debug)]
pub struct CacheBlock {
    /// Pointer to the cached block.
    pub ptr: *mut u8,
    /// Generation counter for reuse tracking.
    pub generation: u32,
}

impl CacheBlock {
    /// Creates a new cache block.
    #[inline]
    pub fn new(ptr: *mut u8, generation: u32) -> Self {
        Self { ptr, generation }
    }

    /// Returns true if this block is valid.
    #[inline]
    pub fn is_valid(&self) -> bool {
        !self.ptr.is_null()
    }
}

/// Per-size-class cache for frequently-used allocation sizes.
#[derive(Clone, Copy, Debug)]
pub struct SizeClassCache {
    /// Bump pointer into the cache buffer.
    pub bump: usize,
    /// Total capacity of this cache.
    pub capacity: usize,
    /// Current generation counter (incremented on eviction).
    pub generation: u32,
    /// Number of hits for cache statistics.
    pub hits: usize,
    /// Number of misses for cache statistics.
    pub misses: usize,
}

impl SizeClassCache {
    /// Creates a new cache with the given capacity.
    #[inline]
    pub const fn new(capacity: usize) -> Self {
        Self {
            bump: 0,
            capacity,
            generation: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// Returns true if the cache has space available.
    #[inline]
    pub fn has_space(&self) -> bool {
        self.bump < self.capacity
    }

    /// Attempts to allocate a block from the cache.
    /// Returns the index if successful, None if cache is full.
    #[inline]
    pub fn allocate(&mut self) -> Option<usize> {
        if self.has_space() {
            let idx = self.bump;
            self.bump += 1;
            self.hits += 1;
            Some(idx)
        } else {
            self.misses += 1;
            None
        }
    }

    /// Resets the cache by clearing the bump pointer and incrementing generation.
    /// This is called when the cache fills up and needs to evict all entries.
    #[inline]
    pub fn reset(&mut self) {
        self.bump = 0;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Returns the cache hit ratio as a percentage (0-100).
    #[inline]
    pub fn hit_ratio(&self) -> u8 {
        let total = self.hits.saturating_add(self.misses);
        if total == 0 {
            0
        } else {
            ((self.hits as u128 * 100) / total as u128) as u8
        }
    }
}

/// Thread-local fast-path cache manager.
///
/// This structure manages multiple per-size-class caches, allowing
/// threads to satisfy allocation requests from thread-local pools
/// without contention on global allocator structures.
pub struct FastPathCacheManager {
    /// Per-size-class caches.
    pub caches: [SizeClassCache; NUM_SIZE_CLASSES],
    /// Configuration for this cache manager.
    pub config: FastPathCacheConfig,
    /// Total allocations served from fast-path.
    pub fast_path_allocations: usize,
    /// Total deallocations through fast-path.
    pub fast_path_deallocations: usize,
    /// Fallback allocations (slow path).
    pub slow_path_allocations: usize,
}

impl FastPathCacheManager {
    /// Creates a new fast-path cache manager with default configuration.
    pub fn new() -> Self {
        Self::with_config(FastPathCacheConfig::default())
    }

    /// Creates a new fast-path cache manager with a custom configuration.
    pub fn with_config(config: FastPathCacheConfig) -> Self {
        let mut caches: [SizeClassCache; NUM_SIZE_CLASSES] =
            [SizeClassCache::new(0); NUM_SIZE_CLASSES];

        if config.enabled {
            for &size_class in config.cached_size_classes {
                if size_class < NUM_SIZE_CLASSES {
                    caches[size_class] = SizeClassCache::new(config.max_blocks_per_class);
                }
            }
        }

        Self {
            caches,
            config,
            fast_path_allocations: 0,
            fast_path_deallocations: 0,
            slow_path_allocations: 0,
        }
    }

    /// Attempts to allocate from the fast-path cache for the given size class.
    /// Returns Some(index) if successful, None if the cache is disabled or full.
    #[inline]
    pub fn try_allocate(&mut self, size_class: usize) -> Option<usize> {
        if !self.config.enabled || size_class >= NUM_SIZE_CLASSES {
            return None;
        }

        // Check if this size class is configured for caching
        if self.caches[size_class].capacity == 0 {
            return None;
        }

        match self.caches[size_class].allocate() {
            Some(idx) => {
                self.fast_path_allocations += 1;
                Some(idx)
            }
            None => {
                self.slow_path_allocations += 1;
                None
            }
        }
    }

    /// Records a deallocation in the fast-path cache.
    #[inline]
    pub fn record_deallocation(&mut self, size_class: usize) {
        if size_class < NUM_SIZE_CLASSES {
            self.fast_path_deallocations += 1;
        }
    }

    /// Resets the cache for a specific size class.
    #[inline]
    pub fn reset_class_cache(&mut self, size_class: usize) {
        if size_class < NUM_SIZE_CLASSES {
            self.caches[size_class].reset();
        }
    }

    /// Resets all caches.
    pub fn reset_all(&mut self) {
        for cache in &mut self.caches {
            if cache.capacity > 0 {
                cache.reset();
            }
        }
    }

    /// Returns cache statistics for a specific size class.
    #[inline]
    pub fn class_stats(&self, size_class: usize) -> Option<(usize, u8)> {
        if size_class < NUM_SIZE_CLASSES {
            let cache = self.caches[size_class];
            Some((cache.hits.saturating_add(cache.misses), cache.hit_ratio()))
        } else {
            None
        }
    }

    /// Returns overall fast-path cache efficiency metrics.
    pub fn efficiency_metrics(&self) -> FastPathEfficiencyMetrics {
        let total_requests = self
            .fast_path_allocations
            .saturating_add(self.slow_path_allocations);
        let fast_path_ratio = if total_requests > 0 {
            ((self.fast_path_allocations as u128 * 100) / total_requests as u128) as u8
        } else {
            0
        };

        let mut avg_cache_hit_ratio = 0u32;
        let mut active_classes = 0usize;
        for cache in &self.caches {
            if cache.capacity > 0 {
                avg_cache_hit_ratio += cache.hit_ratio() as u32;
                active_classes += 1;
            }
        }
        let avg_hit_ratio = if active_classes > 0 {
            (avg_cache_hit_ratio / active_classes as u32) as u8
        } else {
            0
        };

        FastPathEfficiencyMetrics {
            fast_path_allocations: self.fast_path_allocations,
            slow_path_allocations: self.slow_path_allocations,
            fast_path_ratio,
            avg_cache_hit_ratio: avg_hit_ratio,
            total_cache_accesses: total_requests,
        }
    }
}

impl Default for FastPathCacheManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot of fast-path cache efficiency metrics.
#[derive(Clone, Copy, Debug)]
pub struct FastPathEfficiencyMetrics {
    /// Number of allocations served from fast-path caches.
    pub fast_path_allocations: usize,
    /// Number of allocations requiring slow path (miss or disabled).
    pub slow_path_allocations: usize,
    /// Ratio of fast-path allocations as a percentage (0-100).
    pub fast_path_ratio: u8,
    /// Average cache hit ratio across active size classes (0-100).
    pub avg_cache_hit_ratio: u8,
    /// Total cache accesses (hits + misses).
    pub total_cache_accesses: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_block_validity() {
        let ptr = 0x1000 as *mut u8;
        let block = CacheBlock::new(ptr, 0);
        assert!(block.is_valid());

        let null_block = CacheBlock::new(core::ptr::null_mut(), 0);
        assert!(!null_block.is_valid());
    }

    #[test]
    fn test_size_class_cache_allocation() {
        let mut cache = SizeClassCache::new(10);
        assert!(cache.has_space());
        assert_eq!(cache.allocate(), Some(0));
        assert_eq!(cache.allocate(), Some(1));
        assert_eq!(cache.hits, 2);
    }

    #[test]
    fn test_size_class_cache_full() {
        let mut cache = SizeClassCache::new(2);
        assert_eq!(cache.allocate(), Some(0));
        assert_eq!(cache.allocate(), Some(1));
        assert_eq!(cache.allocate(), None);
        assert_eq!(cache.misses, 1);
    }

    #[test]
    fn test_size_class_cache_hit_ratio() {
        let mut cache = SizeClassCache::new(100);
        for _ in 0..80 {
            let _ = cache.allocate();
        }
        cache.misses = 20;
        assert_eq!(cache.hit_ratio(), 80);
    }

    #[test]
    fn test_fast_path_cache_manager() {
        let mut manager = FastPathCacheManager::new();

        // Size class 0 (16 bytes) should be cached
        assert!(manager.try_allocate(0).is_some());
        assert_eq!(manager.fast_path_allocations, 1);

        // Size class 2 (48 bytes) is not in default cached list
        assert!(manager.try_allocate(2).is_none());
    }

    #[test]
    fn test_efficiency_metrics() {
        let mut manager = FastPathCacheManager::new();

        let _ = manager.try_allocate(0);
        let _ = manager.try_allocate(0);

        let metrics = manager.efficiency_metrics();
        assert_eq!(metrics.fast_path_allocations, 2);
        assert_eq!(metrics.total_cache_accesses, 2);
    }
}
