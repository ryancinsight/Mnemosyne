//! Per-NUMA-node huge buckets: the size-bucketed stacks a node retains.
//!
//! [`super::huge_pool::GlobalHugePool`] owns one [`NodeHugePool`] per NUMA
//! node and decides what to retain; these types are the storage it decides
//! over, and they know nothing about that policy.

use super::cache_aligned::CacheAlignedAtomicUsize;
use super::huge_pool::HUGE_SIZE_BUCKETS;
use super::tagged_stack::TaggedSegmentStack;
use mnemosyne_core::types::Segment;

/// Number of ordered bands within one logarithmic huge-size bucket.
pub(super) const HUGE_BUCKET_BANDS: usize = 2;

/// Ordered half-bucket used to avoid scanning known-undersized mappings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HugeBucketBand {
    /// The lower half of the logarithmic bucket.
    Lower,
    /// The upper half of the logarithmic bucket.
    Upper,
}

impl HugeBucketBand {
    const ALL: [Self; HUGE_BUCKET_BANDS] = [Self::Lower, Self::Upper];

    #[inline(always)]
    const fn index(self) -> usize {
        match self {
            Self::Lower => 0,
            Self::Upper => 1,
        }
    }
}

/// A size-bucket for cached huge allocations.
///
/// `Send`/`Sync` are compiler-derived: the bucket holds one tagged stack per
/// ordered band plus its per-band retained-byte counters. The stack's
/// reclamation and tagged-head discipline is documented at the primitive.
pub struct NodeHugeBucket {
    stacks: [TaggedSegmentStack; HUGE_BUCKET_BANDS],
    retained_bytes: [CacheAlignedAtomicUsize; HUGE_BUCKET_BANDS],
}

impl NodeHugeBucket {
    /// Creates a new empty `NodeHugeBucket`.
    pub const fn new() -> Self {
        Self {
            stacks: [const { TaggedSegmentStack::new() }; HUGE_BUCKET_BANDS],
            retained_bytes: [const { CacheAlignedAtomicUsize::new(0) }; HUGE_BUCKET_BANDS],
        }
    }

    #[inline(always)]
    pub(super) fn count(&self) -> usize {
        self.stacks.iter().map(TaggedSegmentStack::len).sum()
    }

    /// Pushes a segment onto the selected ordered-band stack.
    ///
    /// # Safety
    ///
    /// `segment` must be a valid, initialized, and exclusively owned huge
    /// allocation segment. Ownership transfers to this bucket on success.
    #[inline]
    pub(super) unsafe fn push(&self, segment: *mut Segment, band: HugeBucketBand) {
        // SAFETY: the caller's contract guarantees that `segment` is valid and
        // initialized, so its page-0 size is readable before publication.
        let block_size = unsafe { (*segment).pages[0].block_size };
        self.retained_bytes[band.index()]
            .value
            .fetch_add(block_size, core::sync::atomic::Ordering::Relaxed);
        // SAFETY: forwarded contract — `segment` is an exclusively-owned huge
        // `Segment` whose ownership transfers to the stack.
        unsafe { self.stacks[band.index()].push(segment) };
    }

    /// Pops the head segment from one ordered band, if any.
    #[inline]
    pub(super) fn pop_head(&self, band: HugeBucketBand) -> Option<*mut Segment> {
        let popped = self.stacks[band.index()].pop();
        if popped.is_null() {
            None
        } else {
            // SAFETY: a non-null result is exclusively owned by this caller,
            // so its page-0 size remains readable while it leaves the bucket.
            let block_size = unsafe { (*popped).pages[0].block_size };
            self.retained_bytes[band.index()]
                .value
                .fetch_sub(block_size, core::sync::atomic::Ordering::Relaxed);
            Some(popped)
        }
    }

    /// Splices a pre-linked chain of `len` segments onto one ordered-band stack
    /// in a single tagged CAS, preserving the chain's `head → tail` order at
    /// the top of the stack.
    ///
    /// # Safety
    ///
    /// Same contract as [`TaggedSegmentStack::push_chain`]: `head`/`tail` are
    /// non-null, exclusively-owned segments linked through `next_free_segment`
    /// with `tail` reached from `head` in exactly `len - 1` hops; ownership of
    /// every chain node transfers to this bucket. `retained_bytes` must equal
    /// the sum of the page-0 block sizes for all chain nodes.
    #[inline]
    pub(super) unsafe fn push_chain(
        &self,
        band: HugeBucketBand,
        head: *mut Segment,
        tail: *mut Segment,
        len: usize,
        retained_bytes: usize,
    ) {
        self.retained_bytes[band.index()]
            .value
            .fetch_add(retained_bytes, core::sync::atomic::Ordering::Relaxed);
        // SAFETY: forwarded contract — see this method's `# Safety`.
        unsafe { self.stacks[band.index()].push_chain(head, tail, len) };
    }

    #[inline(always)]
    pub(super) fn retained_bytes(&self, band: HugeBucketBand) -> usize {
        self.retained_bytes[band.index()]
            .value
            .load(core::sync::atomic::Ordering::Relaxed)
    }

    /// Detaches both retained band chains in one operation.
    ///
    /// The returned byte count is the exact sum recorded for all detached
    /// chains. Each `(head, count)` pair is one independently detached band.
    #[inline]
    pub(super) fn take_all(&self) -> ([(*mut Segment, usize); HUGE_BUCKET_BANDS], usize) {
        let chains = HugeBucketBand::ALL.map(|band| self.stacks[band.index()].take_all());
        let retained_bytes = HugeBucketBand::ALL
            .into_iter()
            .map(|band| {
                self.retained_bytes[band.index()]
                    .value
                    .swap(0, core::sync::atomic::Ordering::Relaxed)
            })
            .sum();
        (chains, retained_bytes)
    }
}

impl Default for NodeHugeBucket {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// A reclamation-safe pool of cached huge allocations for a single NUMA node.
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
