//! 32-bit fallback implementation of [super::AtomicFreeList].
//!
//! On 32-bit targets there is no room to pack a push counter into the head
//! pointer alongside the block address, so pop_all walks the chain to count
//! elements. This is an O(k) operation, acceptable because 32-bit targets
//! (primarily WASM) do not run under the same throughput pressure as 64-bit
//! hosts.

use super::AtomicFreeList;
use crate::loom_shim::Ordering;
use crate::types::{Block, Segment};
use core::ptr::NonNull;
impl AtomicFreeList {
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
    /// block itself is the mechanism that does (`Block::check_double_free`).
    ///
    /// It is also the only form that keeps a cross-thread free O(1); the walk
    /// made every push cost the length of the queue.
    #[inline]
    fn assert_not_in_queue(&self, block_ptr: *mut Block, _encrypted: bool, _cookie: usize) {
        let head = self.head.load(Ordering::Relaxed);
        if !head.is_null() && head.addr() == block_ptr.addr() {
            crate::abort::abort_on_corruption("Double free detected in AtomicFreeList");
        }
    }

    /// Creates a new empty `AtomicFreeList`.
    pub const fn new() -> Self {
        Self {
            head: crate::loom_shim::AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    /// Pushes a block onto the atomic list.
    ///
    /// This is used for cross-thread deallocation.
    #[inline]
    pub fn push<P: crate::policy::AllocPolicy>(&self, block: NonNull<Block>) {
        self.push_dynamic(block, P::ENABLE_FREE_LIST_ENCRYPTION);
    }

    /// Pushes a block using the encryption mode recorded by its owning
    /// segment. See the 64-bit implementation for the policy-mismatch
    /// rationale.
    #[inline]
    pub fn push_dynamic(&self, block: NonNull<Block>, encrypted: bool) {
        let block_ptr = block.as_ptr();
        // SAFETY: `block` is a live allocation of this allocator, so the
        // segment it lies in is mapped; `locate_segment` only masks its
        // address down to the segment base.
        let (segment, _) = unsafe { crate::types::locate_segment(block_ptr.cast::<u8>()) };
        // SAFETY: `segment` is the live mapping just located.
        if !unsafe { Segment::free_list_mode_matches(segment.cast_const(), encrypted) } {
            crate::abort::abort_on_corruption(
                "free-list mode mismatch: AtomicFreeList push path does not match the segment",
            );
        }
        if !encrypted {
            self.push_raw(block);
            return;
        }

        // SAFETY: as in the 64-bit `push`, `block_ptr` identifies a live block
        // inside its parent segment and therefore carries the mapping
        // provenance needed by `locate_segment` and `cookie_for_dynamic`.
        let cookie = unsafe {
            let (segment, page_index) = crate::types::locate_segment(block_ptr.cast::<u8>());
            Segment::cookie_for_dynamic(segment.cast_const(), encrypted, page_index)
        };

        self.assert_not_in_queue(block_ptr, encrypted, cookie);

        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            if block_ptr == current {
                crate::abort::abort_on_corruption("Double free detected in AtomicFreeList");
            }
            // SAFETY: block_ptr is guaranteed to be valid, writeable, aligned memory,
            // exclusive to the thread calling push.
            unsafe {
                (*block_ptr).set_next_dynamic(NonNull::new(current), encrypted, cookie);
            }
            match self.head.compare_exchange_weak(
                current,
                block_ptr,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Raw push used by the standard-policy hot path, skipping the encrypted
    /// branch and the parent-segment cookie walk.
    ///
    /// This remains internal-only and must only be reached after the owning
    /// segment has already validated that the free-list links are in the raw
    /// mode, or the standard path silently bypasses the encrypted metadata.
    #[inline]
    pub(crate) fn push_raw(&self, block: NonNull<Block>) {
        let block_ptr = block.as_ptr();
        // SAFETY: `block` is a live allocation of this allocator, so the
        // segment it lies in is mapped; `locate_segment` only masks its
        // address down to the segment base.
        let (segment, _) = unsafe { crate::types::locate_segment(block_ptr.cast::<u8>()) };
        // SAFETY: `segment` is the live mapping just located.
        if unsafe { Segment::free_list_encrypted(segment.cast_const()) } {
            crate::abort::abort_on_corruption(
                "raw AtomicFreeList push used while segment free-list links are encrypted",
            );
        }
        self.assert_not_in_queue(block_ptr, false, 0);
        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            if block_ptr == current {
                crate::abort::abort_on_corruption("Double free detected in AtomicFreeList");
            }
            // SAFETY: `block_ptr` is the caller's own block, not yet
            // published to the list, so no other thread can observe it; the
            // raw form matches the segment mode checked on entry.
            unsafe {
                (*block_ptr).set_next_raw(NonNull::new(current));
            }
            match self.head.compare_exchange_weak(
                current,
                block_ptr,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Atomically removes all blocks from the list and returns the head and the count.
    ///
    /// This walks the list to count blocks in O(k).
    #[inline]
    pub fn pop_all(&self, encrypted: bool, cookie: usize) -> Option<(NonNull<Block>, usize)> {
        if !encrypted {
            return self.pop_all_raw();
        }

        let ptr = self.head.swap(core::ptr::null_mut(), Ordering::Acquire);
        NonNull::new(ptr).map(|head| {
            let mut count = 0;
            let mut current = Some(head);
            while let Some(node) = current {
                count += 1;
                if count > crate::constants::PAGE_SIZE {
                    crate::abort::abort_on_corruption("Cycle detected in AtomicFreeList");
                }
                // SAFETY: `node` is `head` or a successor reached through this
                // list, i.e. a block previously published to this `AtomicFreeList`
                // by `push` (a valid, aligned `Block`); the `swap` above gave this
                // thread exclusive ownership of the detached chain, so reading the
                // next-link is sound. The cycle guard above bounds the walk.
                // SAFETY: `node` came from the detached chain, so it is a live
                // block; the mode and cookie are the ones its page recorded.
                current = unsafe { (*node.as_ptr()).get_next_dynamic(encrypted, cookie) };
            }
            (head, count)
        })
    }

    /// Raw pop used by the standard-policy hot path, skipping the encrypted
    /// decode branch while preserving the same detached-chain semantics.
    ///
    /// Internal-only because the caller has already validated the segment is in
    /// the raw free-list mode.
    #[inline]
    pub(crate) fn pop_all_raw(&self) -> Option<(NonNull<Block>, usize)> {
        let ptr = self.head.swap(core::ptr::null_mut(), Ordering::Acquire);
        NonNull::new(ptr).map(|head| {
            let mut count = 0;
            let mut current = Some(head);
            while let Some(node) = current {
                count += 1;
                if count > crate::constants::PAGE_SIZE {
                    crate::abort::abort_on_corruption("Cycle detected in AtomicFreeList");
                }
                // SAFETY: `node` came from the detached chain, so it is a
                // live block, and the raw form matches the mode checked when
                // the chain was taken.
                current = unsafe { (*node.as_ptr()).get_next_raw() };
            }
            (head, count)
        })
    }

    /// Checks if the atomic list is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Relaxed).is_null()
    }
}
