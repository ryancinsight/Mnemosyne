//! Retrieval path for the huge pool: [`GlobalHugePool::pop`] serves a request
//! from the local NUMA node first and then steals, bounded by the
//! over-provision cap, walking undersized bucket heads and restoring rejects
//! in a single splice.

use super::super::node_huge_bucket::{HugeBucketBand, NodeHugeBucket};
use super::super::numa_bucket::{bucket_index as numa_bucket, steal_from};
use super::GlobalHugePool;
use super::bucket::{HUGE_POP_FIT_CAP, HUGE_SIZE_BUCKETS, huge_bucket_band, huge_bucket_index};
use mnemosyne_core::types::Segment;

impl GlobalHugePool {
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
                let block_size = unsafe { (*segment).pages[0].block_size as usize };
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
            let block_size = unsafe { (*segment).pages[0].block_size as usize };
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
}
