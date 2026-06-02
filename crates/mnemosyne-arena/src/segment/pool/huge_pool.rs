use mnemosyne_core::types::Segment;

/// A single lock-free size-bucket for cached huge allocations.
pub struct NodeHugeBucket {
    head: core::sync::atomic::AtomicPtr<Segment>,
    count: core::sync::atomic::AtomicUsize,
}

impl Default for NodeHugeBucket {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl NodeHugeBucket {
    /// Creates a new empty `NodeHugeBucket`.
    pub const fn new() -> Self {
        Self {
            head: core::sync::atomic::AtomicPtr::new(core::ptr::null_mut()),
            count: core::sync::atomic::AtomicUsize::new(0),
        }
    }
}

/// A lock-free pool of cached huge allocations for a single NUMA node, divided into 16 size-buckets.
pub struct NodeHugePool {
    pub(crate) buckets: [NodeHugeBucket; 16],
    pub(crate) total_count: core::sync::atomic::AtomicUsize,
}

impl Default for NodeHugePool {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl NodeHugePool {
    /// Creates a new empty `NodeHugePool`.
    pub const fn new() -> Self {
        Self {
            buckets: [
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
                NodeHugeBucket::new(),
            ],
            total_count: core::sync::atomic::AtomicUsize::new(0),
        }
    }
}

/// A NUMA-aware lock-free global pool of free huge allocations.
pub struct GlobalHugePool {
    nodes: [NodeHugePool; 16],
}

#[inline(always)]
fn huge_bucket_index(size: usize) -> usize {
    let mb = size >> 20;
    if mb >= 16 {
        15
    } else {
        mb
    }
}

impl Default for GlobalHugePool {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalHugePool {
    /// Bounded maximum number of huge allocations cached per NUMA node bucket.
    pub const MAX_CACHED_HUGE_BLOCKS: usize = 1024;
    /// Maximum size class we cache (16MB).
    pub const MAX_CACHED_HUGE_SIZE: usize = 16 * 1024 * 1024;

    /// Creates a new empty `GlobalHugePool` with 16 NUMA node sub-pools.
    pub const fn new() -> Self {
        Self {
            nodes: [
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
                NodeHugePool::new(),
            ],
        }
    }

    /// Pushes a free huge block segment back to the pool if space permits.
    ///
    /// # Safety
    ///
    /// `segment` must point to a valid, initialized, and exclusive `Segment` structure
    /// representing a huge allocation.
    #[inline]
    pub unsafe fn try_push(&self, segment: *mut Segment, numa_node: usize) -> bool {
        let size = unsafe { (*segment).pages[0].block_size };
        if size > Self::MAX_CACHED_HUGE_SIZE {
            return false;
        }

        let node = numa_node % 16;
        let bucket_idx = huge_bucket_index(size);
        let pool_node = &self.nodes[node];
        let bucket = &pool_node.buckets[bucket_idx];

        let count = bucket.count.load(core::sync::atomic::Ordering::Relaxed);
        if count >= Self::MAX_CACHED_HUGE_BLOCKS {
            return false;
        }

        let mut head = bucket.head.load(core::sync::atomic::Ordering::Relaxed);
        loop {
            // Write next pointer into the segment header
            unsafe {
                (*segment).next_free_segment = head;
            }

            match bucket.head.compare_exchange_weak(
                head,
                segment,
                core::sync::atomic::Ordering::Release,
                core::sync::atomic::Ordering::Relaxed,
            ) {
                Ok(_) => {
                    bucket
                        .count
                        .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    pool_node
                        .total_count
                        .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
                    return true;
                }
                Err(actual) => head = actual,
            }
        }
    }

    /// Pops a huge block segment from the pool that is at least `size` bytes, stealing if needed.
    ///
    /// # Safety
    ///
    /// The returned segment is exclusively owned by the caller.
    #[inline]
    pub unsafe fn pop<B: mnemosyne_core::MemoryBackend>(
        &self,
        size: usize,
        numa_node: usize,
    ) -> Option<*mut Segment> {
        let start_node = numa_node % 16;
        let bucket_idx = huge_bucket_index(size);

        // 1. Try local NUMA node first
        if self.nodes[start_node]
            .total_count
            .load(core::sync::atomic::Ordering::Relaxed)
            > 0
        {
            if let Some(res) = self.pop_from_node::<B>(size, start_node, bucket_idx) {
                return Some(res);
            }
        }

        // 2. Steal from other nodes
        for i in 1..16 {
            let other_node = (start_node + i) % 16;
            if self.nodes[other_node]
                .total_count
                .load(core::sync::atomic::Ordering::Relaxed)
                > 0
            {
                if let Some(res) = self.pop_from_node::<B>(size, other_node, bucket_idx) {
                    return Some(res);
                }
            }
        }

        None
    }

    #[inline]
    unsafe fn pop_from_node<B: mnemosyne_core::MemoryBackend>(
        &self,
        size: usize,
        node: usize,
        start_bucket: usize,
    ) -> Option<*mut Segment> {
        let pool_node = &self.nodes[node];
        if pool_node
            .total_count
            .load(core::sync::atomic::Ordering::Relaxed)
            == 0
        {
            return None;
        }

        for bucket_idx in start_bucket..16 {
            let bucket = &pool_node.buckets[bucket_idx];
            let mut head = bucket.head.load(core::sync::atomic::Ordering::Acquire);
            loop {
                if head.is_null() {
                    break;
                }

                let next = unsafe { (*head).next_free_segment };
                let block_size = unsafe { (*head).pages[0].block_size };

                if block_size >= size {
                    match bucket.head.compare_exchange_weak(
                        head,
                        next,
                        core::sync::atomic::Ordering::AcqRel,
                        core::sync::atomic::Ordering::Acquire,
                    ) {
                        Ok(_) => {
                            bucket
                                .count
                                .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                            pool_node
                                .total_count
                                .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                            return Some(head);
                        }
                        Err(actual) => head = actual,
                    }
                } else {
                    // If it is too small, pop it anyway, deallocate it to OS, and continue.
                    match bucket.head.compare_exchange_weak(
                        head,
                        next,
                        core::sync::atomic::Ordering::AcqRel,
                        core::sync::atomic::Ordering::Acquire,
                    ) {
                        Ok(_) => {
                            bucket
                                .count
                                .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                            pool_node
                                .total_count
                                .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                            let raw_ptr = unsafe { (*head).raw_alloc_ptr };
                            let _ = unsafe { B::deallocate(raw_ptr, block_size) };
                            head = bucket.head.load(core::sync::atomic::Ordering::Acquire);
                        }
                        Err(actual) => head = actual,
                    }
                }
            }
        }
        None
    }

    /// Purges all cached huge blocks and releases them to the OS.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the backend `B` is valid and that no threads
    /// are concurrently accessing the purged memory or segment pointers.
    pub unsafe fn purge<B: mnemosyne_core::MemoryBackend>(&self) {
        for node in 0..16 {
            let pool_node = &self.nodes[node];
            pool_node
                .total_count
                .store(0, core::sync::atomic::Ordering::Relaxed);
            for bucket_idx in 0..16 {
                let bucket = &pool_node.buckets[bucket_idx];
                let mut head = bucket
                    .head
                    .swap(core::ptr::null_mut(), core::sync::atomic::Ordering::Acquire);
                bucket.count.store(0, core::sync::atomic::Ordering::Relaxed);

                while !head.is_null() {
                    let next = unsafe { (*head).next_free_segment };
                    let raw_ptr = unsafe { (*head).raw_alloc_ptr };
                    let block_size = unsafe { (*head).pages[0].block_size };
                    let _ = unsafe { B::deallocate(raw_ptr, block_size) };
                    head = next;
                }
            }
        }
    }
}
