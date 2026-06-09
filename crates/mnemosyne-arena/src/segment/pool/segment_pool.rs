use crate::numa::current_numa_node;
use crate::segment::pool::list::NodeSegmentPool;
use mnemosyne_core::types::Segment;
use themis::NumaNodeId;

const NUMA_BUCKETS: usize = 16;

#[inline(always)]
fn numa_bucket(node: u32) -> usize {
    NumaNodeId::new(node).bucket_index::<NUMA_BUCKETS>().index()
}

/// A NUMA-aware lock-free global pool of free segments partitioned by socket node.
pub struct GlobalSegmentPool {
    nodes: [NodeSegmentPool; NUMA_BUCKETS],
}

impl GlobalSegmentPool {
    /// Creates a new empty `GlobalSegmentPool` with 16 NUMA node sub-pools.
    pub const fn new() -> Self {
        Self {
            nodes: [
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
                NodeSegmentPool::new(),
            ],
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
        // Local node cache miss: refresh our TLS cached NUMA node ID in case we migrated.
        let new_node = numa_bucket(crate::numa::refresh_numa_node());
        if new_node != node {
            node = new_node;
            if let Some(segment) = self.nodes[node].pop() {
                return Some(segment);
            }
        }
        // 2. Steal from other nodes
        let start = NumaNodeId::new(node as u32).bucket_index::<NUMA_BUCKETS>();
        for i in 1..NUMA_BUCKETS {
            let other = start.wrapping_add(i).index();
            if let Some(segment) = self.nodes[other].pop() {
                return Some(segment);
            }
        }
        None
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
