use super::cache_aligned::CacheAlignedAtomicUsize;
use super::numa_bucket::{NUMA_BUCKETS, bucket_from_usize as numa_bucket, steal_from};
use super::tagged_stack::TaggedSegmentStack;
use mnemosyne_core::types::Segment;

/// Number of huge size buckets: the bucket index of the largest cacheable size
/// ([`GlobalHugePool::MAX_CACHED_HUGE_SIZE`]) plus one.
///
/// `try_push` rejects anything larger than `MAX_CACHED_HUGE_SIZE`, so buckets
/// beyond that index would be permanently unreachable dead statics (and wasted
/// count-line reads on every pop miss). Deriving the count from the SSOT pins
/// the fan-out to the cacheable range; the const assertion below enforces that
/// the max cacheable size maps to the last bucket.
pub(crate) const HUGE_SIZE_BUCKETS: usize =
    log2_ceil_bucket_index(GlobalHugePool::MAX_CACHED_HUGE_SIZE) + 1;

/// A size-bucket for cached huge allocations.
///
/// `Send`/`Sync` are compiler-derived: the bucket holds only the atomics of
/// the `TaggedSegmentStack`, whose lock-free / ABA-tag discipline is documented
/// at the primitive.
pub struct NodeHugeBucket {
    stack: TaggedSegmentStack,
}

impl NodeHugeBucket {
    /// Creates a new empty `NodeHugeBucket`.
    pub const fn new() -> Self {
        Self {
            stack: TaggedSegmentStack::new(),
        }
    }

    #[inline(always)]
    fn count(&self) -> usize {
        self.stack.len()
    }

    /// Pushes a segment onto this bucket's Treiber stack.
    ///
    /// # Safety
    ///
    /// `segment` must be a valid, initialized, and exclusively owned huge
    /// allocation segment. Ownership transfers to this bucket on success.
    #[inline]
    unsafe fn push(&self, segment: *mut Segment) {
        // SAFETY: forwarded contract — `segment` is an exclusively-owned huge
        // `Segment` whose ownership transfers to the stack.
        unsafe { self.stack.push(segment) };
    }

    /// Pops the head segment from this bucket, if any.
    #[inline]
    fn pop_head(&self) -> Option<*mut Segment> {
        let popped = self.stack.pop();
        if popped.is_null() { None } else { Some(popped) }
    }

    /// Splices a pre-linked chain of `len` segments onto this bucket's Treiber
    /// stack in a single tagged CAS, preserving the chain's `head → tail` order
    /// at the top of the stack.
    ///
    /// # Safety
    ///
    /// Same contract as [`TaggedSegmentStack::push_chain`]: `head`/`tail` are
    /// non-null, exclusively-owned segments linked through `next_free_segment`
    /// with `tail` reached from `head` in exactly `len - 1` hops; ownership of
    /// every chain node transfers to this bucket.
    #[inline]
    unsafe fn push_chain(&self, head: *mut Segment, tail: *mut Segment, len: usize) {
        // SAFETY: forwarded contract — see this method's `# Safety`.
        unsafe { self.stack.push_chain(head, tail, len) };
    }

    /// Detaches this bucket's full retained chain in one atomic operation.
    #[inline]
    fn take_all(&self) -> (*mut Segment, usize) {
        self.stack.take_all()
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
    pub(crate) total_count: CacheAlignedAtomicUsize,
    /// Advisory total bytes of huge blocks retained on this node, maintained
    /// with one `Relaxed` add/sub alongside every `total_count` update.
    pub(crate) total_bytes: CacheAlignedAtomicUsize,
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
            buckets: [const { NodeHugeBucket::new() }; HUGE_SIZE_BUCKETS],
            total_count: CacheAlignedAtomicUsize::new(0),
            total_bytes: CacheAlignedAtomicUsize::new(0),
        }
    }
}

/// A NUMA-aware lock-free global pool of free huge allocations.
pub struct GlobalHugePool {
    nodes: [NodeHugePool; NUMA_BUCKETS],
}

/// Unclamped log2-ceil bucket index: sizes `<= 16 KiB` map to bucket 0;
/// otherwise bucket `b` covers `(2^(b+13), 2^(b+14)]` bytes.
///
/// This is the raw bucketing math that also defines [`HUGE_SIZE_BUCKETS`];
/// callers use [`huge_bucket_index`], which clamps to the live bucket range.
const fn log2_ceil_bucket_index(size: usize) -> usize {
    if size <= 16384 {
        0
    } else {
        let bits = usize::BITS - (size - 1).leading_zeros();
        (bits as usize).saturating_sub(14)
    }
}

#[inline(always)]
pub(crate) const fn huge_bucket_index(size: usize) -> usize {
    let idx = log2_ceil_bucket_index(size);
    if idx >= HUGE_SIZE_BUCKETS {
        HUGE_SIZE_BUCKETS - 1
    } else {
        idx
    }
}

// Pin the SSOT derivation: the largest cacheable size maps to the last bucket,
// so exactly `huge_bucket_index(MAX_CACHED_HUGE_SIZE) + 1` buckets are live.
const _: () =
    assert!(huge_bucket_index(GlobalHugePool::MAX_CACHED_HUGE_SIZE) == HUGE_SIZE_BUCKETS - 1);

/// Upward-scan over-provision cap factor for cache pops.
///
/// `pop_from_node` serves a request from a bucket above the request's own only
/// while that bucket's smallest possible block (`2^(bucket_idx+13) + 1` bytes —
/// bucket `b` covers `(2^(b+13), 2^(b+14)]`) does not exceed
/// `HUGE_POP_FIT_CAP ×` the requested total size. Because a bucket's largest
/// block is less than 2× its exclusive lower bound, a cache hit then
/// over-provisions the request by less than `2 × HUGE_POP_FIT_CAP = 8×` in the
/// worst case, while still permitting reuse across adjacent size classes.
/// Without the cap, a ~20 KiB-class request could be satisfied by a cached
/// 16 MiB block (~800× over-provision) whose slack stays committed, because
/// the cache-hit allocation path skips slack decommit. Buckets beyond the cap
/// are skipped without popping: the bucket index lower-bounds every block a
/// bucket holds, so none of them can satisfy the cap.
pub(crate) const HUGE_POP_FIT_CAP: usize = 4;

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

        if bucket.count() >= bucket_block_cap(bucket_idx) {
            return false;
        }

        // SAFETY: by this function's contract, ownership of `segment`
        // transfers to the pool on a successful cache insertion.
        unsafe {
            bucket.push(segment);
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

    /// Pops a huge block segment from the pool that is at least `size` bytes, stealing if needed.
    ///
    /// The block returned is bounded above by the `HUGE_POP_FIT_CAP`
    /// over-provision cap (a private crate constant, 4): buckets whose
    /// smallest block exceeds `HUGE_POP_FIT_CAP × size` are never used, so an
    /// oversized cached block misses (returns `None`) rather than
    /// over-committing RSS.
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
        // SAFETY: `pop_from_node` returns an exclusively-owned segment on
        // success, matching this function's ownership contract.
        if let Some(res) = unsafe { self.pop_from_node(size, start_node, bucket_idx) } {
            return Some(res);
        }

        steal_from(start_node, |other_node| {
            // SAFETY: `pop_from_node` returns an exclusively-owned segment on
            // success; this closure only chooses the NUMA node traversal order.
            unsafe { self.pop_from_node(size, other_node, bucket_idx) }
        })
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
            .value
            .load(core::sync::atomic::Ordering::Relaxed)
            == 0
        {
            return None;
        }

        for bucket_idx in start_bucket..HUGE_SIZE_BUCKETS {
            // Fit cap: stop scanning once the bucket's smallest possible block
            // (its exclusive lower bound `2^(bucket_idx+13)` plus one byte)
            // would over-provision the request beyond `HUGE_POP_FIT_CAP ×`.
            // Buckets are monotonic in block size, so every higher bucket is
            // also inadmissible — no popping needed to know it cannot fit.
            // `saturating_mul` degrades to "no cap" for astronomically large
            // requests, which exceed `MAX_CACHED_HUGE_SIZE` and miss anyway.
            if bucket_idx > start_bucket
                && (1usize << (bucket_idx + 13)) >= size.saturating_mul(HUGE_POP_FIT_CAP)
            {
                break;
            }

            let bucket = &pool_node.buckets[bucket_idx];
            if bucket.count() == 0 {
                continue;
            }

            let popped = if bucket_idx == start_bucket {
                // SAFETY: this method owns each temporarily detached segment
                // until it either returns a fit or restores the rejected chain.
                unsafe { Self::pop_fitting_from_exact_bucket(bucket, size) }
            } else {
                // Higher bucket: every retained block is at least `size`.
                bucket.pop_head()
            };

            if let Some(segment) = popped {
                // SAFETY: the pop transferred exclusive ownership of `segment`
                // to this caller, so reading its page-0 `block_size` is sound.
                let block_size = unsafe { (*segment).pages[0].block_size };
                pool_node
                    .total_count
                    .value
                    .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
                pool_node
                    .total_bytes
                    .value
                    .fetch_sub(block_size, core::sync::atomic::Ordering::Relaxed);
                return Some(segment);
            }
        }
        None
    }

    /// Pops the first segment of at least `size` bytes from `bucket`, walking
    /// past undersized heads.
    ///
    /// Rejected segments are collected into a private chain during the walk —
    /// head, tail, and length tracked as they are popped — and restored with a
    /// single [`NodeHugeBucket::push_chain`] splice: one CAS total instead of
    /// one retriable CAS per rejected node on a contended head line. Because
    /// the walk pops from the stack head and appends each reject at the private
    /// chain's tail, the splice reinstalls the rejects in their original
    /// relative order above whatever remains on the stack, so the bucket order
    /// is unchanged apart from the extracted fit.
    #[inline]
    unsafe fn pop_fitting_from_exact_bucket(
        bucket: &NodeHugeBucket,
        size: usize,
    ) -> Option<*mut Segment> {
        let mut rejected_head: *mut Segment = core::ptr::null_mut();
        let mut rejected_tail: *mut Segment = core::ptr::null_mut();
        let mut rejected_len = 0usize;

        let mut fit = None;
        while let Some(segment) = bucket.pop_head() {
            // SAFETY: `pop_head` transfers exclusive ownership of `segment`.
            let block_size = unsafe { (*segment).pages[0].block_size };
            if block_size >= size {
                fit = Some(segment);
                break;
            }

            // Append the reject at the private chain's tail, preserving walk
            // order. `pop_head` already cleared `segment`'s own link, so the
            // chain stays null-terminated at `rejected_tail`.
            if rejected_tail.is_null() {
                rejected_head = segment;
            } else {
                // SAFETY: `rejected_tail` was removed from the shared stack by
                // this walk and is exclusively owned until the splice below.
                unsafe {
                    (*rejected_tail).next_free_segment = segment;
                }
            }
            rejected_tail = segment;
            rejected_len += 1;
        }

        if !rejected_head.is_null() {
            // SAFETY: every rejected segment was removed from the shared stack
            // and linked only through this private chain; `rejected_head` /
            // `rejected_tail` delimit exactly `rejected_len` nodes, whose
            // ownership transfers back to the bucket in one CAS.
            unsafe {
                bucket.push_chain(rejected_head, rejected_tail, rejected_len);
            }
        }
        fit
    }

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
                let (mut head, count) = bucket.take_all();
                if count == 0 {
                    continue;
                }
                pool_node
                    .total_count
                    .value
                    .fetch_sub(count, core::sync::atomic::Ordering::Relaxed);

                let mut released_bytes = 0usize;
                while !head.is_null() {
                    // SAFETY: `head` is a segment detached from this bucket by
                    // `take_all` and is no longer reachable by any other thread
                    // (the caller guarantees no concurrent access during purge),
                    // so it is exclusively owned here. Reading its links/size and
                    // releasing its recorded mapping through the allocating
                    // backend `B` is sound; `next` is captured before the mapping
                    // is freed.
                    let next = unsafe {
                        let next = (*head).next_free_segment;
                        let raw_ptr = (*head).raw_alloc_ptr;
                        let block_size = (*head).pages[0].block_size;
                        released_bytes += block_size;
                        let _ = B::deallocate(raw_ptr, block_size);
                        next
                    };
                    head = next;
                }
                pool_node
                    .total_bytes
                    .value
                    .fetch_sub(released_bytes, core::sync::atomic::Ordering::Relaxed);
            }
        }
    }
}
