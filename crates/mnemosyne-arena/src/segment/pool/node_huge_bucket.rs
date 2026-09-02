//! Per-NUMA-node huge buckets: the size-bucketed stacks a node retains.
//!
//! [`super::huge_pool::GlobalHugePool`] owns one [`NodeHugePool`] per NUMA
//! node and decides what to retain; these types are the storage it decides
//! over, and they know nothing about that policy.

use super::cache_aligned::CacheAlignedAtomicUsize;
use super::huge_pool::HUGE_SIZE_BUCKETS;
use super::tagged_stack::TaggedSegmentStack;
use mnemosyne_core::types::Segment;

/// A size-bucket for cached huge allocations.
///
/// `Send`/`Sync` are compiler-derived: the bucket holds only the atomics of
/// the `TaggedSegmentStack`, whose reclamation and tagged-head discipline is documented
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
    pub(super) fn count(&self) -> usize {
        self.stack.len()
    }

    /// Pushes a segment onto this bucket's intrusive stack.
    ///
    /// # Safety
    ///
    /// `segment` must be a valid, initialized, and exclusively owned huge
    /// allocation segment. Ownership transfers to this bucket on success.
    #[inline]
    pub(super) unsafe fn push(&self, segment: *mut Segment) {
        // SAFETY: forwarded contract — `segment` is an exclusively-owned huge
        // `Segment` whose ownership transfers to the stack.
        unsafe { self.stack.push(segment) };
    }

    /// Pops the head segment from this bucket, if any.
    #[inline]
    pub(super) fn pop_head(&self) -> Option<*mut Segment> {
        let popped = self.stack.pop();
        if popped.is_null() { None } else { Some(popped) }
    }

    /// Splices a pre-linked chain of `len` segments onto this bucket's intrusive
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
    pub(super) unsafe fn push_chain(&self, head: *mut Segment, tail: *mut Segment, len: usize) {
        // SAFETY: forwarded contract — see this method's `# Safety`.
        unsafe { self.stack.push_chain(head, tail, len) };
    }

    /// Detaches this bucket's full retained chain in one atomic operation.
    #[inline]
    pub(super) fn take_all(&self) -> (*mut Segment, usize) {
        self.stack.take_all()
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
