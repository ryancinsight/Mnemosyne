//! Retained huge mappings, bucketed by size class within each NUMA node.

// The bucket types stay reachable at their original path: this module is
// `pub`, so moving them out of the file would have removed
// `pool::huge_pool::NodeHugeBucket` from the public surface — a break the
// semver gate flagged, and not one a file-size refactor may make.
use super::node_huge_bucket::HugeBucketBand;
pub use super::node_huge_bucket::{NodeHugeBucket, NodeHugePool};
use super::numa_bucket::{NUMA_BUCKETS, bucket_index as numa_bucket, steal_from};
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

/// A NUMA-aware reclamation-safe global pool of free huge allocations.
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

/// Selects the ordered half of a logarithmic bucket for `size`.
///
/// The lower band ends at the bucket midpoint. A request in the upper band
/// cannot be satisfied by any lower-band mapping, so exact-bucket lookup can
/// skip that stack instead of walking and restoring known-undersized blocks.
#[inline(always)]
const fn huge_bucket_band(size: usize, bucket_idx: usize) -> HugeBucketBand {
    let lower_bound = 1usize << (bucket_idx + 13);
    let midpoint = lower_bound + lower_bound / 2;
    if size > midpoint {
        HugeBucketBand::Upper
    } else {
        HugeBucketBand::Lower
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
    /// Per-bucket retained-byte budget. Admission enforces this budget against
    /// actual block sizes, so mixed-size buckets use available capacity without
    /// allowing a single bucket to retain more than ~256 MiB of idle mappings.
    pub const MAX_CACHED_HUGE_BYTES_PER_BUCKET: usize = 256 * 1024 * 1024;
    /// Per-band retained-byte budget. The two bands partition the bucket budget
    /// so a saturated lower band cannot starve upper-band reuse.
    pub(crate) const MAX_CACHED_HUGE_BYTES_PER_BAND: usize =
        Self::MAX_CACHED_HUGE_BYTES_PER_BUCKET / super::node_huge_bucket::HUGE_BUCKET_BANDS;

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
        let start_node = numa_bucket(
            u32::try_from(numa_node).expect("invariant: NUMA node identifiers fit in u32"),
        );
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

        let requested_band = huge_bucket_band(size, start_bucket);

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
                match requested_band {
                    HugeBucketBand::Lower => {
                        let lower = unsafe {
                            Self::pop_fitting_from_exact_bucket(bucket, size, HugeBucketBand::Lower)
                        };
                        match lower {
                            Some(segment) => Some(segment),
                            // SAFETY: `bucket` is a live pool bucket and the
                            // helper transfers exclusive ownership only when
                            // it finds a fitting retained segment.
                            None => unsafe {
                                Self::pop_fitting_from_exact_bucket(
                                    bucket,
                                    size,
                                    HugeBucketBand::Upper,
                                )
                            },
                        }
                    }
                    // SAFETY: same ownership window as the Lower-band call
                    // above — this method still owns the temporarily detached
                    // chain (the Lower pop returned None and restored it), so
                    // popping an Upper-band fit from the detached bucket is
                    // valid; a rejected walk is restored before return.
                    HugeBucketBand::Upper => unsafe {
                        Self::pop_fitting_from_exact_bucket(bucket, size, HugeBucketBand::Upper)
                    },
                }
            } else {
                // Higher bucket: every retained block is at least `size`.
                match bucket.pop_head(HugeBucketBand::Lower) {
                    Some(segment) => Some(segment),
                    None => bucket.pop_head(HugeBucketBand::Upper),
                }
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
        band: HugeBucketBand,
    ) -> Option<*mut Segment> {
        let mut rejected_head: *mut Segment = core::ptr::null_mut();
        let mut rejected_tail: *mut Segment = core::ptr::null_mut();
        let mut rejected_len = 0usize;
        let mut rejected_bytes = 0usize;

        let mut fit = None;
        while let Some(segment) = bucket.pop_head(band) {
            // SAFETY: `pop_head` transfers exclusive ownership of `segment`.
            let block_size = unsafe { (*segment).pages[0].block_size };
            if block_size >= size {
                fit = Some(segment);
                break;
            }

            rejected_bytes += block_size;
            // Append the reject at the private chain's tail, preserving walk
            // order. `pop_head` already cleared `segment`'s own link, so the
            // chain stays null-terminated at `rejected_tail`.
            if rejected_tail.is_null() {
                rejected_head = segment;
            } else {
                // SAFETY: `rejected_tail` was removed from the shared stack by
                // this walk and is exclusively owned until the splice below.
                unsafe {
                    (*rejected_tail)
                        .next_free_segment
                        .store(segment, core::sync::atomic::Ordering::Relaxed);
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
                bucket.push_chain(
                    band,
                    rejected_head,
                    rejected_tail,
                    rejected_len,
                    rejected_bytes,
                );
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
                            let block_size = (*head).pages[0].block_size;
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
