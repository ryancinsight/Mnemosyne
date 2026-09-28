//! [`DeferredChain`] — segments held back during thread teardown for later placement.
//!
//! A chain of segments that could not be handed to any pool without waiting
//! is accumulated here and pushed back once the locks are available. The
//! chain is linked through `next_free_segment`, which is unused while a
//! segment belongs to a thread cache.

use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::types::Segment;
/// Segments held back during thread teardown because no sink would accept them
/// without waiting, linked through `next_free_segment` for one batched
/// placement.
///
/// The field is free to borrow: it is the global pools' link, unused for as
/// long as a segment belongs to a thread cache, and every node here has already
/// been unlinked from that cache.
pub(crate) struct DeferredChain {
    /// Most recently deferred node; null when the chain is empty.
    head: *mut Segment,
    /// First deferred node, whose `next_free_segment` terminates the chain.
    tail: *mut Segment,
    len: usize,
}

impl DeferredChain {
    #[inline]
    pub(crate) const fn new() -> Self {
        Self {
            head: core::ptr::null_mut(),
            tail: core::ptr::null_mut(),
            len: 0,
        }
    }

    /// Prepends `segment` to the chain.
    ///
    /// # Safety
    ///
    /// `segment` must be a live `Segment` exclusively owned by the caller and
    /// unreachable from any pool; ownership transfers to the chain.
    #[inline]
    pub(crate) unsafe fn push(&mut self, segment: *mut Segment) {
        // SAFETY: `segment` is live and exclusively owned per the contract, so
        // writing its pool link is unobservable to any other thread.
        unsafe {
            (*segment)
                .next_free_segment
                .store(self.head, core::sync::atomic::Ordering::Relaxed);
        }
        if self.head.is_null() {
            self.tail = segment;
        }
        self.head = segment;
        self.len += 1;
    }

    /// Hands the whole chain to `B`'s orphan pool in one acquisition.
    ///
    /// # Safety
    ///
    /// Every node must still be exclusively owned by the caller with its owner
    /// identity cleared; ownership transfers to the pool.
    #[inline]
    pub(crate) unsafe fn place_in_orphan_pool<B: HasSegmentPool>(self) {
        if self.head.is_null() {
            return;
        }
        let pool = B::global_orphan_pool();
        // SAFETY: `push` built a `len`-node chain from `head` to `tail` through
        // `next_free_segment`, exclusively owned here per the contract, which is
        // exactly the pool's chain-push contract.
        unsafe {
            if !pool.try_push_chain_unbounded(self.head, self.tail, self.len) {
                // The only wait left in the teardown, and it covers the whole
                // chain rather than one segment: a node still holding live
                // allocations cannot be unmapped and the orphan pool is its
                // only sink, so the alternative here is to leak it.
                pool.push_chain_unbounded(self.head, self.tail, self.len);
            }
        }
    }
}
