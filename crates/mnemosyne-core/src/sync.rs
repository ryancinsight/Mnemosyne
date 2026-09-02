//! Synchronization primitives for the allocator, including lock-free structures.

use crate::loom_shim::Ordering;
use crate::types::{Block, Segment};
use core::ptr::NonNull;

/// A lock-free, atomic singly-linked list of blocks.
///
/// Implements atomic push and atomic pop-all operations, matching the deallocation
/// queue pattern from mimalloc.
pub struct AtomicFreeList {
    head: crate::loom_shim::AtomicPtr<Block>,
}

/// On 64-bit targets the head is a single `AtomicPtr` that packs the list head
/// address (low bits) with a wrapping push counter (high bits), so
/// `pop_all` returns the block count in O(1) without walking the list.
/// `ptr::map_addr` changes only the address component, retaining the head
/// block's provenance through atomic publication and consumption.
///
/// Layout: bits `0..PACKED_PTR_BITS` hold the head block address; the
/// remaining high bits hold a push counter.
///
/// # Portability contract
///
/// The packing assumes every block address fits in `PACKED_PTR_BITS` (48) bits.
/// That holds for mainstream 64-bit userspace targets: x86-64 and AArch64
/// canonical low-half addresses use at most 48 bits under 4-level paging, and
/// Linux/Windows keep default `mmap`/`VirtualAlloc` allocations below `2^47`
/// even when 5-level paging (LA57) or large VAs are enabled. `push` enforces
/// the invariant in every build: an address that does not fit aborts through
/// `abort_on_corruption`, it is not a debug-only assertion that disappears in
/// release. The counter cannot wrap in practice because
/// a page holds at most `PAGE_SIZE / MIN_BLOCK_SIZE` (<= 4096) blocks, far
/// below the counter's `2^(64 - PACKED_PTR_BITS)` capacity. The 32-bit fallback
/// `impl` below stores a bare `AtomicPtr` and counts in O(k).
#[cfg(target_pointer_width = "64")]
impl AtomicFreeList {
    /// Low bits reserved for the packed block address.
    const PACKED_PTR_BITS: u32 = 48;
    /// Mask selecting the packed address bits.
    const PTR_MASK: usize = (1usize << Self::PACKED_PTR_BITS) - 1;
    /// Mask wrapping the push counter to the remaining high bits.
    const COUNT_WRAP_MASK: usize = (1usize << (usize::BITS - Self::PACKED_PTR_BITS)) - 1;

    /// Creates a new empty `AtomicFreeList`.
    ///
    /// `const` in ordinary builds so `Page::new()` can stay const. Loom's
    /// instrumented atomics are not const-constructible, so the model build
    /// gets a non-const form; nothing in the shipped allocator changes.
    #[cfg(not(loom))]
    pub const fn new() -> Self {
        Self {
            head: crate::loom_shim::AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    /// Loom-build constructor. See the `cfg(not(loom))` form above.
    #[cfg(loom)]
    pub fn new() -> Self {
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
    /// segment.
    ///
    /// Cross-thread frees may be issued under a different policy type than
    /// the policy that created the block. The segment's mode is therefore the
    /// SSOT for this operation; selecting from the freeing caller's `P` would
    /// recreate the mixed-chain corruption AR-1 prevents.
    #[inline]
    pub fn push_dynamic(&self, block: NonNull<Block>, encrypted: bool) {
        let block_ptr = block.as_ptr();
        let block_addr = block_ptr.addr();
        if (block_addr & !Self::PTR_MASK) != 0 {
            crate::abort::abort_on_corruption("Block address does not fit in 48 bits");
        }

        // SAFETY: `block` is a live allocation, so `locate_segment` on its
        // pointer recovers the valid parent segment header and its in-range page
        // index, satisfying `cookie_for`'s contract.
        let cookie = unsafe {
            let (segment, page_index) = crate::types::locate_segment(block_ptr.cast::<u8>());
            Segment::cookie_for_dynamic(segment.cast_const(), encrypted, page_index)
        };

        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            let current_value = current.addr();
            let current_addr = current_value & Self::PTR_MASK;
            if block_addr == current_addr {
                crate::abort::abort_on_corruption("Double free detected in AtomicFreeList");
            }
            let current_ptr = current.map_addr(|_| current_addr);
            let next_count = ((current_value >> Self::PACKED_PTR_BITS) + 1) & Self::COUNT_WRAP_MASK;

            // SAFETY: block_ptr is valid, writeable, aligned memory, exclusive
            // to the pushing thread until the CAS publishes it.
            unsafe {
                (*block_ptr).set_next_dynamic(NonNull::new(current_ptr), encrypted, cookie);
            }

            let next_val = (next_count << Self::PACKED_PTR_BITS) | block_addr;
            let next = block_ptr.map_addr(|_| next_val);

            match self.head.compare_exchange_weak(
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

    /// Atomically removes all blocks from the list and returns the head and the count.
    ///
    /// This is wait-free and returns a standard local linked list along with its count in O(1).
    #[inline]
    pub fn pop_all(&self, _encrypted: bool, _cookie: usize) -> Option<(NonNull<Block>, usize)> {
        let val = self.head.swap(core::ptr::null_mut(), Ordering::Acquire);
        let packed = val.addr();
        let addr = packed & Self::PTR_MASK;
        let count = packed >> Self::PACKED_PTR_BITS;
        let ptr = val.map_addr(|_| addr);
        NonNull::new(ptr).map(|head| (head, count))
    }

    /// Checks if the atomic list is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        (self.head.load(Ordering::Relaxed).addr() & Self::PTR_MASK) == 0
    }
}

#[cfg(not(target_pointer_width = "64"))]
impl AtomicFreeList {
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
        // SAFETY: as in the 64-bit `push`, `block_ptr` identifies a live block
        // inside its parent segment and therefore carries the mapping
        // provenance needed by `locate_segment` and `cookie_for_dynamic`.
        let cookie = unsafe {
            let (segment, page_index) = crate::types::locate_segment(block_ptr.cast::<u8>());
            Segment::cookie_for_dynamic(segment.cast_const(), encrypted, page_index)
        };

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

    /// Atomically removes all blocks from the list and returns the head and the count.
    ///
    /// This walks the list to count blocks in O(k).
    #[inline]
    pub fn pop_all(&self, encrypted: bool, cookie: usize) -> Option<(NonNull<Block>, usize)> {
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
                current = unsafe { (*node.as_ptr()).get_next_dynamic(encrypted, cookie) };
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

impl Default for AtomicFreeList {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
