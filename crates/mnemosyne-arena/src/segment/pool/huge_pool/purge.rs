//! Reclamation path for the huge pool: [`GlobalHugePool::purge`] drains every
//! NUMA-node bucket and releases the retained mappings to the OS through the
//! allocating backend.

use super::super::numa_bucket::NUMA_BUCKETS;
use super::GlobalHugePool;
use super::bucket::HUGE_SIZE_BUCKETS;

impl GlobalHugePool {
    /// Purges all cached huge blocks and releases them to the OS.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the backend `B` is valid and that no threads
    /// are concurrently accessing the purged memory or segment pointers.
    pub unsafe fn purge<B: mnemosyne_core::MemoryBackend>(&self) {
        for node in 0..NUMA_BUCKETS {
            let pool_node = &self.nodes[node];
            for bucket_idx in 0..HUGE_SIZE_BUCKETS {
                let bucket = &pool_node.buckets[bucket_idx];
                let (chains, retained_bytes) = bucket.take_all();
                for (mut head, count) in chains {
                    if count == 0 {
                        continue;
                    }
                    pool_node
                        .total_count
                        .value
                        .fetch_sub(count, core::sync::atomic::Ordering::Relaxed);

                    while !head.is_null() {
                        // SAFETY: `head` is a segment detached from this bucket by
                        // `take_all` and is no longer reachable by any other thread
                        // (the caller guarantees no concurrent access during purge),
                        // so it is exclusively owned here. Reading its links/size and
                        // releasing its recorded mapping through the allocating
                        // backend `B` is sound; `next` is captured before the mapping
                        // is freed.
                        let next = unsafe {
                            let next = (*head)
                                .next_free_segment
                                .load(core::sync::atomic::Ordering::Relaxed);
                            let raw_ptr = (*head).raw_alloc_ptr;
                            let block_size = (*head).pages[0].block_size as usize;
                            let _ = B::deallocate(raw_ptr, block_size);
                            next
                        };
                        head = next;
                    }
                }
                pool_node
                    .total_bytes
                    .value
                    .fetch_sub(retained_bytes, core::sync::atomic::Ordering::Relaxed);
            }
        }
    }
}
