use mnemosyne_core::types::Segment;
use themis::NumaNodeId;

const NUMA_BUCKETS: usize = 16;
const HUGE_SIZE_BUCKETS: usize = 16;

#[inline(always)]
fn numa_bucket(node: usize) -> usize {
    NumaNodeId::new(node as u32)
        .bucket_index::<NUMA_BUCKETS>()
        .index()
}

struct NodeHugeBucketState {
    head: *mut Segment,
}

/// A size-bucket for cached huge allocations.
#[repr(align(64))]
pub struct NodeHugeBucket {
    lock: mnemosyne_core::sync::SpinLock,
    state: core::cell::UnsafeCell<NodeHugeBucketState>,
    count: core::sync::atomic::AtomicUsize,
}

// SAFETY: the raw `head` pointer inside the `UnsafeCell` state is only ever
// accessed while the bucket's `SpinLock` is held, which serializes all reads and
// writes across threads; the `count` atomic is independently synchronized. That
// discipline makes shared cross-thread access (`Sync`) and ownership transfer
// (`Send`) of a `NodeHugeBucket` data-race free.
unsafe impl Send for NodeHugeBucket {}
unsafe impl Sync for NodeHugeBucket {}

impl NodeHugeBucket {
    /// Creates a new empty `NodeHugeBucket`.
    pub const fn new() -> Self {
        Self {
            lock: mnemosyne_core::sync::SpinLock::new(),
            state: core::cell::UnsafeCell::new(NodeHugeBucketState {
                head: core::ptr::null_mut(),
            }),
            count: core::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl Default for NodeHugeBucket {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// A lock-free pool of cached huge allocations for a single NUMA node.
pub struct NodeHugePool {
    pub(crate) buckets: [NodeHugeBucket; HUGE_SIZE_BUCKETS],
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
    nodes: [NodeHugePool; NUMA_BUCKETS],
}

#[inline(always)]
pub(crate) fn huge_bucket_index(size: usize) -> usize {
    if size <= 16384 {
        0
    } else {
        let bits = usize::BITS - (size - 1).leading_zeros();
        let idx = (bits as usize).saturating_sub(14);
        if idx >= HUGE_SIZE_BUCKETS {
            HUGE_SIZE_BUCKETS - 1
        } else {
            idx
        }
    }
}

/// Per-bucket retained-block cap, derived from a per-bucket byte budget.
///
/// A flat `MAX_CACHED_HUGE_BLOCKS` count cap lets a large-size bucket retain
/// `1024 × (block size)` bytes — up to ~16 GiB for the 16 MiB bucket. Capping
/// each bucket's *retained bytes* instead (block count ≤ budget / max block size
/// in the bucket) bounds idle RSS while leaving the small-huge buckets at the
/// full count cap, so the warm cache for the common case is preserved. Bucket
/// `b` holds sizes in `(2^(b+13), 2^(b+14)]`, so `2^(b+14)` upper-bounds a block.
#[inline(always)]
pub(crate) const fn bucket_block_cap(bucket_idx: usize) -> usize {
    let max_block = 1usize << (bucket_idx + 14);
    let cap = GlobalHugePool::MAX_CACHED_HUGE_BYTES_PER_BUCKET / max_block;
    if cap == 0 {
        1
    } else if cap > GlobalHugePool::MAX_CACHED_HUGE_BLOCKS {
        GlobalHugePool::MAX_CACHED_HUGE_BLOCKS
    } else {
        cap
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
    /// Per-bucket retained-byte budget. The effective per-bucket block cap is
    /// `min(MAX_CACHED_HUGE_BLOCKS, this / max-block-size-in-bucket)`, so a single
    /// node bucket never retains more than ~256 MiB of idle huge mappings (vs. up
    /// to ~16 GiB under a flat count cap). The private `bucket_block_cap`
    /// helper derives the effective per-bucket limit.
    pub const MAX_CACHED_HUGE_BYTES_PER_BUCKET: usize = 256 * 1024 * 1024;

    /// Creates a new empty `GlobalHugePool` with `NUMA_BUCKETS` node sub-pools.
    pub const fn new() -> Self {
        // Derive the array length from the `NUMA_BUCKETS` SSOT rather than a
        // hand-written literal, so the fan-out can never drift from the constant.
        Self {
            nodes: [const { NodeHugePool::new() }; NUMA_BUCKETS],
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
        // SAFETY: by this function's contract `segment` is a valid, initialized,
        // exclusively-owned huge-allocation `Segment`, so reading its page-0
        // `block_size` is sound.
        let size = unsafe { (*segment).pages[0].block_size };
        if size > Self::MAX_CACHED_HUGE_SIZE {
            return false;
        }

        let node = numa_bucket(numa_node);
        let bucket_idx = huge_bucket_index(size);
        let pool_node = &self.nodes[node];
        let bucket = &pool_node.buckets[bucket_idx];

        bucket.lock.lock();
        let count = bucket.count.load(core::sync::atomic::Ordering::Relaxed);
        if count >= bucket_block_cap(bucket_idx) {
            bucket.lock.unlock();
            return false;
        }

        // Safety: We hold the spinlock, so we have exclusive access to the state.
        unsafe {
            let state = &mut *bucket.state.get();
            (*segment).next_free_segment = state.head;
            state.head = segment;
        }

        bucket
            .count
            .store(count + 1, core::sync::atomic::Ordering::Relaxed);
        pool_node
            .total_count
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        bucket.lock.unlock();
        true
    }

    /// Pops a huge block segment from the pool that is at least `size` bytes, stealing if needed.
    ///
    /// # Safety
    ///
    /// The returned segment is exclusively owned by the caller.
    #[inline]
    pub unsafe fn pop(&self, size: usize, numa_node: usize) -> Option<*mut Segment> {
        let start_node = numa_bucket(numa_node);
        let bucket_idx = huge_bucket_index(size);

        // `pop_from_node` already early-returns on an empty node (its leading
        // `total_count == 0` check), so a redundant pre-load here would only
        // re-read the same atomic. Call it directly: local node first, then steal.
        if let Some(res) = self.pop_from_node(size, start_node, bucket_idx) {
            return Some(res);
        }

        let start = NumaNodeId::new(start_node as u32).bucket_index::<NUMA_BUCKETS>();
        for i in 1..NUMA_BUCKETS {
            let other_node = start.wrapping_add(i).index();
            if let Some(res) = self.pop_from_node(size, other_node, bucket_idx) {
                return Some(res);
            }
        }

        None
    }

    #[inline]
    unsafe fn pop_from_node(
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

        for bucket_idx in start_bucket..HUGE_SIZE_BUCKETS {
            let bucket = &pool_node.buckets[bucket_idx];
            if bucket.count.load(core::sync::atomic::Ordering::Relaxed) == 0 {
                continue;
            }
            bucket.lock.lock();
            // SAFETY: the bucket spinlock is held, granting exclusive access to
            // its `UnsafeCell` state and to the intrusive `next_free_segment`
            // links of every cached segment reachable from `state.head`; each
            // segment was pushed as a valid, exclusively-owned huge `Segment`.
            unsafe {
                let state = &mut *bucket.state.get();
                if bucket_idx == start_bucket {
                    // Search for the first block that fits.
                    let mut prev: *mut Segment = core::ptr::null_mut();
                    let mut curr = state.head;
                    while !curr.is_null() {
                        let block_size = (*curr).pages[0].block_size;
                        if block_size >= size {
                            // Unlink curr
                            if prev.is_null() {
                                state.head = (*curr).next_free_segment;
                            } else {
                                (*prev).next_free_segment = (*curr).next_free_segment;
                            }
                            (*curr).next_free_segment = core::ptr::null_mut();
                            bucket
                                .count
                                .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                            pool_node
                                .total_count
                                .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                            bucket.lock.unlock();
                            return Some(curr);
                        }
                        prev = curr;
                        curr = (*curr).next_free_segment;
                    }
                } else {
                    // Higher bucket: first block is guaranteed to fit.
                    let h = state.head;
                    if !h.is_null() {
                        state.head = (*h).next_free_segment;
                        (*h).next_free_segment = core::ptr::null_mut();
                        bucket
                            .count
                            .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                        pool_node
                            .total_count
                            .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                        bucket.lock.unlock();
                        return Some(h);
                    }
                }
            }
            bucket.lock.unlock();
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
        for node in 0..NUMA_BUCKETS {
            let pool_node = &self.nodes[node];
            for bucket_idx in 0..HUGE_SIZE_BUCKETS {
                let bucket = &pool_node.buckets[bucket_idx];
                bucket.lock.lock();
                let count = bucket.count.load(core::sync::atomic::Ordering::Relaxed);
                if count == 0 {
                    bucket.lock.unlock();
                    continue;
                }
                // SAFETY: the bucket spinlock is held, so accessing its
                // `UnsafeCell` state is exclusive; detaching the list head leaves
                // the bucket empty for the drain below.
                let mut head = unsafe {
                    let state = &mut *bucket.state.get();
                    let h = state.head;
                    state.head = core::ptr::null_mut();
                    h
                };
                bucket.count.store(0, core::sync::atomic::Ordering::Relaxed);
                pool_node
                    .total_count
                    .fetch_sub(count, core::sync::atomic::Ordering::Relaxed);
                bucket.lock.unlock();

                while !head.is_null() {
                    // SAFETY: `head` is a segment detached from this bucket under
                    // the lock and is no longer reachable by any other thread (the
                    // caller guarantees no concurrent access during purge), so it
                    // is exclusively owned here. Reading its links/size and
                    // releasing its recorded mapping through the allocating backend
                    // `B` is sound; `next` is captured before the mapping is freed.
                    let next = unsafe {
                        let next = (*head).next_free_segment;
                        let raw_ptr = (*head).raw_alloc_ptr;
                        let block_size = (*head).pages[0].block_size;
                        let _ = B::deallocate(raw_ptr, block_size);
                        next
                    };
                    head = next;
                }
            }
        }
    }
}
