//! Admission path for the huge pool: [`GlobalHugePool::try_push`] returns a
//! free huge mapping to its NUMA-node size bucket when the advisory budgets
//! permit.

use super::super::numa_bucket::bucket_index as numa_bucket;
use super::GlobalHugePool;
use super::bucket::{huge_bucket_band, huge_bucket_index};
use mnemosyne_core::types::Segment;

impl GlobalHugePool {
    /// Pushes a free huge block segment back to the pool if space permits.
    ///
    /// # Safety
    ///
    /// `segment` must point to a valid, initialized, and exclusive `Segment` structure
    /// representing a huge allocation.
    #[inline]
    pub unsafe fn try_push(&self, segment: *mut Segment, numa_node: usize) -> bool {
        // SAFETY: by this function's contract `segment` is a valid, initialized,
        // exclusively-owned huge-allocation `Segment`, so reading its page-0
        // `block_size` is sound.
        let size = unsafe { (*segment).pages[0].block_size as usize };
        if size > Self::MAX_CACHED_HUGE_SIZE {
            return false;
        }

        let node = numa_bucket(
            u32::try_from(numa_node).expect("invariant: NUMA node identifiers fit in u32"),
        );
        let bucket_idx = huge_bucket_index(size);
        let pool_node = &self.nodes[node];
        let bucket = &pool_node.buckets[bucket_idx];
        let band = huge_bucket_band(size, bucket_idx);

        // Soft limit check, matching `NodeSegmentPool::try_push_retained`: the
        // count and byte readings are advisory, so concurrent pushers can both
        // pass this gate and overshoot the per-bucket budgets (and the node
        // totals below) by at most the number of racing pushers. A cache can
        // accept that bound; the alternative — holding a lock to atomically
        // check and push — is the contention this path exists to avoid.
        let retained_bytes = bucket.retained_bytes(band);
        if bucket.count() >= Self::MAX_CACHED_HUGE_BLOCKS
            || size > Self::MAX_CACHED_HUGE_BYTES_PER_BAND.saturating_sub(retained_bytes)
        {
            return false;
        }

        // SAFETY: by this function's contract, ownership of `segment`
        // transfers to the pool on a successful cache insertion.
        unsafe {
            bucket.push(segment, band);
        }

        pool_node
            .total_count
            .value
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        pool_node
            .total_bytes
            .value
            .fetch_add(size, core::sync::atomic::Ordering::Relaxed);
        true
    }
}
