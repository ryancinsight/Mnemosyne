use crate::numa::current_numa_node;
use crate::segment::pool::list::NodeSegmentPool;
use crate::segment::pool::numa_bucket::{bucket_from_u32 as numa_bucket, steal_from, NUMA_BUCKETS};
use mnemosyne_core::types::Segment;

/// A NUMA-aware lock-free global pool of free segments partitioned by socket node.
pub struct GlobalSegmentPool {
    nodes: [NodeSegmentPool; NUMA_BUCKETS],
}

impl GlobalSegmentPool {
    /// Creates a new empty `GlobalSegmentPool` with `NUMA_BUCKETS` node sub-pools.
    pub const fn new() -> Self {
        // Derive the array length from the `NUMA_BUCKETS` SSOT rather than a
        // hand-written literal, so the fan-out can never drift from the constant.
        Self {
            nodes: [const { NodeSegmentPool::new() }; NUMA_BUCKETS],
        }
    }

    /// Pushes a segment back to the correct NUMA node pool without applying a retention limit.
    ///
    /// # Safety
    ///
    /// The `segment` pointer must be a valid, initialized, and exclusive pointer to a
    /// `Segment` structure.
    #[inline]
    pub unsafe fn push_unbounded(&self, segment: *mut Segment) {
        // SAFETY: by this function's contract `segment` is a valid, initialized,
        // exclusively-owned `Segment`, so reading its `numa_node` field is sound.
        let node = numa_bucket(unsafe { (*segment).numa_node });
        self.nodes[node].push_unbounded(segment);
    }

    /// Pushes a segment back to its originating NUMA node pool if retention limit permits.
    ///
    /// # Safety
    ///
    /// The `segment` pointer must be a valid, initialized, and exclusive pointer to a
    /// `Segment` structure.
    #[inline]
    pub unsafe fn try_push_retained(&self, segment: *mut Segment) -> bool {
        // SAFETY: by this function's contract `segment` is a valid, initialized,
        // exclusively-owned `Segment`, so reading its `numa_node` field is sound.
        let node = numa_bucket(unsafe { (*segment).numa_node });
        self.nodes[node].try_push_retained(segment)
    }

    /// Pops a segment from the calling thread's NUMA node pool, stealing from other nodes if empty.
    #[inline]
    pub fn pop(&self) -> Option<*mut Segment> {
        let mut node = numa_bucket(current_numa_node());
        // 1. Try local node first
        if let Some(segment) = self.nodes[node].pop() {
            return Some(segment);
        }
        // Local node cache miss: refresh our TLS cached NUMA node ID in case we migrated,
        // but rate-limit the OS query to avoid system call overhead under high contention/miss rates.
        #[cfg(feature = "std")]
        let mut refreshed = false;
        #[cfg(feature = "std")]
        std::thread_local! {
            static MISS_COUNT: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
        }
        #[cfg(feature = "std")]
        let new_node = MISS_COUNT.with(|c| {
            let count = c.get();
            if count >= 31 {
                c.set(0);
                refreshed = true;
                numa_bucket(crate::numa::refresh_numa_node())
            } else {
                c.set(count + 1);
                node
            }
        });
        #[cfg(not(feature = "std"))]
        let new_node = numa_bucket(crate::numa::refresh_numa_node());

        #[cfg(feature = "std")]
        if refreshed && new_node != node {
            node = new_node;
            if let Some(segment) = self.nodes[node].pop() {
                return Some(segment);
            }
        }
        #[cfg(not(feature = "std"))]
        if new_node != node {
            node = new_node;
            if let Some(segment) = self.nodes[node].pop() {
                return Some(segment);
            }
        }
        // 2. Steal from other nodes.
        steal_from(node, |other| self.nodes[other].pop())
    }

    /// The per-NUMA-node sub-pools, for sweeps that detach each node's chain
    /// under a single lock (see [`NodeSegmentPool::take_all`]).
    #[inline]
    pub(crate) fn nodes(&self) -> &[NodeSegmentPool] {
        &self.nodes
    }

    #[inline]
    pub fn retained_count(&self) -> usize {
        self.nodes.iter().map(|n| n.retained_count()).sum()
    }

    #[inline]
    pub fn purged_count(&self) -> usize {
        self.nodes.iter().map(|n| n.purged_count()).sum()
    }

    #[inline]
    pub fn purge_call_count(&self) -> usize {
        self.nodes.iter().map(|n| n.purge_call_count()).sum()
    }

    #[inline]
    pub(crate) fn record_purge(&self, count: usize) {
        let node = numa_bucket(current_numa_node());
        self.nodes[node].record_purge(count);
    }

    #[inline]
    pub fn reset_segments_count(&self) -> usize {
        self.nodes.iter().map(|n| n.reset_segments_count()).sum()
    }

    #[inline]
    pub fn reset_call_count(&self) -> usize {
        self.nodes.iter().map(|n| n.reset_call_count()).sum()
    }

    #[inline]
    pub(crate) fn record_reset(&self, count: usize) {
        let node = numa_bucket(current_numa_node());
        self.nodes[node].record_reset(count);
    }
}

impl Default for GlobalSegmentPool {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
