//! [`AtomicFreeList`]: the lock-free cross-thread deallocation queue.
//!
//! One implementation parameterized by [`HeadCodec`], the target's head-word
//! encoding (`super::wide::PackedHead` on 64-bit, `super::narrow::BareHead`
//! otherwise). The push CAS loop, the double-free head check, and the
//! draining swap are written once against the codec, so the two encodings
//! cannot drift apart.

use crate::loom_shim::{AtomicPtr, Ordering};
use crate::types::{Block, FreedBlock, Segment};
use core::ptr::NonNull;

#[cfg(target_pointer_width = "64")]
use super::wide::PackedHead as SelectedHead;

#[cfg(not(target_pointer_width = "64"))]
use super::narrow::BareHead as SelectedHead;

/// The target's head-word encoding for [`AtomicFreeList`].
///
/// Only the pure encoding lives here; every operation with a loop or a policy
/// decision (the CAS push, the double-free head check, the draining swap) is
/// written once in the [`AtomicFreeList`] impl parameterized by this ZST.
pub(crate) trait HeadCodec: 'static {
    /// Whether a drain must walk the chain to recover the block count.
    ///
    /// `true` for the bare codec, whose head word stores no counter.
    const COUNT_REQUIRES_WALK: bool;

    /// Reads the head word with `order`.
    fn load(head: &AtomicPtr<Block>, order: Ordering) -> *mut Block;

    /// Attempts to replace head word `current` with `next`.
    fn cas(
        head: &AtomicPtr<Block>,
        current: *mut Block,
        next: *mut Block,
        success: Ordering,
        failure: Ordering,
    ) -> Result<*mut Block, *mut Block>;

    /// Swaps the head word with null and returns the old word.
    fn swap_null(head: &AtomicPtr<Block>, order: Ordering) -> *mut Block;

    /// The block address a head word names (null when the list is empty).
    fn addr(raw: *mut Block) -> *mut Block;

    /// Aborts if `addr` cannot be encoded as a head word.
    fn assert_packable(addr: *mut Block);

    /// Wraps `addr` as a head word, advancing `current`'s tag/count.
    fn pack(addr: *mut Block, current: *mut Block) -> *mut Block;

    /// Aborts if a detached head word violates the encoding's own invariant.
    fn validate(raw: *mut Block);

    /// The number of blocks a detached `raw` chain holds.
    ///
    /// `walked` is the number of nodes the caller walked, or `None` when the
    /// caller skipped the walk because the encoding stores its count. A codec
    /// with `COUNT_REQUIRES_WALK` is only ever called with `Some`.
    fn count_of(raw: *mut Block, walked: Option<usize>) -> usize;
}

/// A lock-free, atomic singly-linked list of blocks.
///
/// Implements atomic push and atomic pop-all operations, matching the
/// deallocation queue pattern from mimalloc. The 64-bit encoding packs a push
/// counter into the high bits of the head pointer so `pop_all` can return the
/// block count in O(1); the 32-bit encoding stores a bare pointer and walks the
/// chain.
pub struct AtomicFreeList {
    pub(crate) head: AtomicPtr<crate::types::Block>,
}

impl Default for AtomicFreeList {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl AtomicFreeList {
    /// Creates a new empty `AtomicFreeList`.
    ///
    /// `const` in ordinary builds so `Page::new()` can stay const. Loom's
    /// instrumented atomics are not const-constructible, so the model build
    /// gets a non-const form; nothing in the shipped allocator changes.
    #[cfg(not(loom))]
    pub const fn new() -> Self {
        Self {
            head: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    /// Loom-build constructor. See the `cfg(not(loom))` form above.
    #[cfg(loom)]
    pub fn new() -> Self {
        Self {
            head: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    /// Pushes a block onto the atomic list.
    ///
    /// This is used for cross-thread deallocation.
    #[inline]
    pub fn push<P: crate::policy::AllocPolicy>(&self, block: FreedBlock) {
        self.push_dynamic(block, P::ENABLE_FREE_LIST_ENCRYPTION);
    }

    /// Pushes a block using the encryption mode recorded by its owning
    /// segment.
    ///
    /// Cross-thread frees may be issued under a different policy type than the
    /// policy that created the block. The segment's mode is therefore the SSOT
    /// for this operation; selecting from the freeing caller's `P` would
    /// recreate the mixed-chain corruption AR-1 prevents.
    #[inline]
    pub fn push_dynamic(&self, block: FreedBlock, encrypted: bool) {
        self.push_dynamic_with::<SelectedHead>(block, encrypted);
    }

    /// Raw push used by the free-list unit tests to exercise the non-encrypted
    /// path directly.
    ///
    /// The shipped paths never call this: `push_dynamic` reaches
    /// `push_raw_with` through its codec parameter, so this wrapper exists only
    /// for the tests and is compiled only for them. It must only be reached
    /// after the segment's `free_list_encrypted` flag has already been checked;
    /// otherwise the raw path bypasses the XOR metadata entirely and
    /// reintroduces the mode-mismatch invariant the encrypted policy is designed
    /// to prevent.
    #[cfg(test)]
    #[inline]
    pub(crate) fn push_raw(&self, block: NonNull<Block>) {
        // SAFETY: test blocks are owned whole through their mapping pointer.
        let block = unsafe { FreedBlock::new(block, block.as_ptr().cast(), usize::MAX) };
        self.push_raw_with::<SelectedHead>(block);
    }

    /// Atomically removes all blocks from the list and returns the head and the
    /// count.
    ///
    /// The encoded count is read in O(1); the detached chain is still walked to
    /// detect a cycle and to cross-check that count against the chain length.
    #[inline]
    pub fn pop_all(&self, encrypted: bool, cookie: usize) -> Option<(NonNull<Block>, usize)> {
        self.drain::<SelectedHead, true>(encrypted, cookie)
    }

    /// Raw pop used by the non-encrypted hot path, avoiding the branch and the
    /// unused cookie argument entirely.
    ///
    /// This remains internal-only, because the caller must already have verified
    /// that the page's free-list links are not encrypted.
    #[inline]
    pub(crate) fn pop_all_raw(&self) -> Option<(NonNull<Block>, usize)> {
        self.drain::<SelectedHead, false>(false, 0)
    }

    /// Checks if the atomic list is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        SelectedHead::addr(SelectedHead::load(&self.head, Ordering::Relaxed)).is_null()
    }

    /// Aborts when `block_ptr` is already the queue's head.
    ///
    /// The check is the head and only the head. Walking the chain would read
    /// each block's `next` link, and those links are written non-atomically by
    /// whichever thread pushed them -- a concurrent walk is a data race, which
    /// is what Miri and ThreadSanitizer both reported here. The head is an
    /// atomic load and races with nothing, and it is where a double push lands:
    /// the second push of a block sees the first still on top. A block pushed
    /// twice with other pushes interleaved escapes this guard, and no
    /// race-free O(1) check catches that case -- the free-canary check on the
    /// block itself is the mechanism that does (`FreedBlock::has_free_canary`).
    ///
    /// It is also the only form that keeps a cross-thread free O(1); the walk
    /// made every push cost the length of the queue.
    #[inline]
    fn assert_not_in_queue<C: HeadCodec>(&self, block_ptr: *mut Block) {
        let head_ptr = C::addr(C::load(&self.head, Ordering::Relaxed));
        if !head_ptr.is_null() && head_ptr == C::addr(block_ptr) {
            crate::abort::abort_on_corruption("Double free detected in AtomicFreeList");
        }
    }

    /// The shared CAS push loop: link `block_ptr` to the observed head, then
    /// publish with a `Release` CAS.
    ///
    /// `set_next` writes `block_ptr`'s next-link for the observed head before
    /// publication; monomorphization keeps this zero-cost. A `Relaxed` failure
    /// ordering is sound because the failure value is only re-stored into a
    /// block this thread exclusively owns, never dereferenced.
    #[inline]
    fn push_loop<C: HeadCodec>(&self, block_ptr: *mut Block, mut set_next: impl FnMut(*mut Block)) {
        let mut current = C::load(&self.head, Ordering::Relaxed);
        loop {
            let current_ptr = C::addr(current);
            if current_ptr == block_ptr {
                crate::abort::abort_on_corruption("Double free detected in AtomicFreeList");
            }
            set_next(current_ptr);
            let next = C::pack(block_ptr, current);
            match C::cas(
                &self.head,
                current,
                next,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Body of [`Self::push_dynamic`] for head codec `C`.
    #[inline]
    fn push_dynamic_with<C: HeadCodec>(&self, block: FreedBlock, encrypted: bool) {
        let block_ptr = block.block().as_ptr();
        let (segment, page_index) = block.segment();
        // SAFETY: `block` is a live allocation of this allocator, so its
        // segment header is live.
        if !unsafe { Segment::free_list_mode_matches(segment.cast_const(), encrypted) } {
            crate::abort::abort_on_corruption(
                "free-list mode mismatch: AtomicFreeList push path does not match the segment",
            );
        }
        if !encrypted {
            self.push_raw_with::<C>(block);
            return;
        }

        C::assert_packable(block_ptr);

        // SAFETY: `segment` is the block's live parent segment header and
        // `page_index` its in-range page index, satisfying `cookie_for`'s
        // contract.
        let cookie =
            unsafe { Segment::cookie_for_dynamic(segment.cast_const(), encrypted, page_index) };

        self.assert_not_in_queue::<C>(block_ptr);
        self.push_loop::<C>(block_ptr, |current_ptr| {
            // SAFETY: the block is exclusive to the pushing thread until the
            // CAS publishes it; `cookie` is its segment's key for this mode.
            unsafe { block.set_next_dynamic(NonNull::new(current_ptr), encrypted, cookie) };
        });
    }

    /// Body of [`Self::push_raw`] for head codec `C`.
    #[inline]
    fn push_raw_with<C: HeadCodec>(&self, block: FreedBlock) {
        let block_ptr = block.block().as_ptr();
        let (segment, _) = block.segment();
        // SAFETY: `block` is a live allocation of this allocator, so its
        // segment header is live.
        if unsafe { Segment::free_list_encrypted(segment.cast_const()) } {
            crate::abort::abort_on_corruption(
                "raw AtomicFreeList push used while segment free-list links are encrypted",
            );
        }

        C::assert_packable(block_ptr);

        self.assert_not_in_queue::<C>(block_ptr);
        self.push_loop::<C>(block_ptr, |current_ptr| {
            // SAFETY: the block is the caller's own, not yet published, so no
            // other thread can observe it; the raw form matches the segment
            // mode checked on entry.
            unsafe { block.set_next_dynamic(NonNull::new(current_ptr), false, 0) };
        });
    }

    /// Detaches the whole chain and returns its head (or `None`) and count.
    ///
    /// `VERIFY` requests a chain walk even when the codec stores its count, so
    /// the encoded count is cross-checked against the chain length. A codec
    /// that cannot recover its count without a walk
    /// ([`HeadCodec::COUNT_REQUIRES_WALK`]) always walks.
    #[inline]
    fn drain<C: HeadCodec, const VERIFY: bool>(
        &self,
        encrypted: bool,
        cookie: usize,
    ) -> Option<(NonNull<Block>, usize)> {
        let raw = C::swap_null(&self.head, Ordering::Acquire);
        C::validate(raw);
        let head = NonNull::new(C::addr(raw))?;

        let walked = if C::COUNT_REQUIRES_WALK || VERIFY {
            Some(walked_chain_len(head, encrypted, cookie))
        } else {
            None
        };
        let count = C::count_of(raw, walked);
        Some((head, count))
    }
}

/// Walks a detached chain, returning its node count and rejecting a cycle.
///
/// # Panics
///
/// Aborts the process via [`crate::abort::abort_on_corruption`] if the chain
/// exceeds `PAGE_SIZE` nodes, which can only mean a corrupted cycle.
#[inline]
fn walked_chain_len(head: NonNull<Block>, encrypted: bool, cookie: usize) -> usize {
    let mut walked = 0usize;
    let mut current = Some(head);
    while let Some(node) = current {
        walked += 1;
        if walked > crate::constants::PAGE_SIZE {
            crate::abort::abort_on_corruption("Cycle detected in AtomicFreeList");
        }
        // SAFETY: `node` came from the detached chain, so it is a live block;
        // the mode and cookie are the ones its page recorded.
        current = unsafe {
            if encrypted {
                (*node.as_ptr()).get_next_dynamic(encrypted, cookie)
            } else {
                (*node.as_ptr()).get_next_raw()
            }
        };
    }
    walked
}
