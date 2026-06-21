//! Synchronization primitives for the allocator, including lock-free structures.

use crate::types::Block;
use core::ptr::NonNull;
use core::sync::atomic::Ordering;

/// A lock-free, atomic singly-linked list of blocks.
///
/// Implements atomic push and atomic pop-all operations, matching the deallocation
/// queue pattern from mimalloc.
#[cfg(target_pointer_width = "64")]
pub struct AtomicFreeList {
    head: core::sync::atomic::AtomicUsize,
}

#[cfg(not(target_pointer_width = "64"))]
pub struct AtomicFreeList {
    head: core::sync::atomic::AtomicPtr<Block>,
}

/// On 64-bit targets the head is a single `AtomicUsize` that packs the list
/// head address (low bits) with a wrapping push counter (high bits), so
/// `pop_all` returns the block count in O(1) without walking the list.
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
/// even when 5-level paging (LA57) or large VAs are enabled. `push`
/// `debug_assert!`s the invariant. The counter cannot wrap in practice because
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
    pub const fn new() -> Self {
        Self {
            head: core::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Pushes a block onto the atomic list.
    ///
    /// This is used for cross-thread deallocation.
    #[inline]
    pub fn push<P: crate::policy::AllocPolicy>(&self, block: NonNull<Block>) {
        let block_ptr = block.as_ptr();
        // A tagged-pointer free list inherently materializes pointers from
        // integers, so it uses *exposed* provenance (it cannot be
        // strict-provenance clean — Miri flags exposed provenance the same as a
        // bare cast). Using the explicit `expose_provenance` /
        // `with_exposed_provenance_mut` APIs rather than bare `as` casts states
        // that intent precisely and keeps the round-trip well-defined under the
        // exposed-provenance model.
        let block_addr = block_ptr.expose_provenance();
        debug_assert_eq!(
            block_addr & !Self::PTR_MASK,
            0,
            "block address {:p} does not fit in {} bits; the packed deallocation \
             queue requires canonical low-half userspace addresses",
            block_ptr,
            Self::PACKED_PTR_BITS
        );

        let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
            let segment_addr = block_addr & !(crate::constants::SEGMENT_SIZE - 1);
            let page_index =
                (block_addr & (crate::constants::SEGMENT_SIZE - 1)) >> crate::constants::PAGE_SHIFT;
            unsafe { (*(segment_addr as *const crate::types::Segment)).keys[page_index] }
        } else {
            0
        };

        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            let current_addr = current & Self::PTR_MASK;
            let current_ptr = core::ptr::with_exposed_provenance_mut::<Block>(current_addr);
            let next_count = ((current >> Self::PACKED_PTR_BITS) + 1) & Self::COUNT_WRAP_MASK;

            // Safety: block_ptr is valid, writeable, aligned memory, exclusive
            // to the pushing thread until the CAS publishes it.
            unsafe {
                (*block_ptr).set_next::<P>(NonNull::new(current_ptr), cookie);
            }

            let next_val = (next_count << Self::PACKED_PTR_BITS) | block_addr;

            match self.head.compare_exchange_weak(
                current,
                next_val,
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
        let val = self.head.swap(0, Ordering::Acquire);
        let addr = val & Self::PTR_MASK;
        let count = val >> Self::PACKED_PTR_BITS;
        let ptr = core::ptr::with_exposed_provenance_mut::<Block>(addr);
        NonNull::new(ptr).map(|head| (head, count))
    }

    /// Checks if the atomic list is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        (self.head.load(Ordering::Relaxed) & Self::PTR_MASK) == 0
    }
}

#[cfg(not(target_pointer_width = "64"))]
impl AtomicFreeList {
    /// Creates a new empty `AtomicFreeList`.
    pub const fn new() -> Self {
        Self {
            head: core::sync::atomic::AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    /// Pushes a block onto the atomic list.
    ///
    /// This is used for cross-thread deallocation.
    #[inline]
    pub fn push<P: crate::policy::AllocPolicy>(&self, block: NonNull<Block>) {
        let block_ptr = block.as_ptr();
        let block_addr = block_ptr as usize;
        let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
            let segment_addr = block_addr & !(crate::constants::SEGMENT_SIZE - 1);
            let page_index =
                (block_addr & (crate::constants::SEGMENT_SIZE - 1)) >> crate::constants::PAGE_SHIFT;
            unsafe { (*(segment_addr as *const crate::types::Segment)).keys[page_index] }
        } else {
            0
        };

        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            // Safety: block_ptr is guaranteed to be valid, writeable, aligned memory,
            // exclusive to the thread calling push.
            unsafe {
                (*block_ptr).set_next::<P>(NonNull::new(current), cookie);
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

/// A lightweight, allocation-free spinlock.
pub struct SpinLock {
    locked: core::sync::atomic::AtomicBool,
}

impl Default for SpinLock {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl SpinLock {
    /// Creates a new empty `SpinLock`.
    pub const fn new() -> Self {
        Self {
            locked: core::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Acquires the lock, spinning if necessary.
    #[inline(always)]
    pub fn lock(&self) {
        while self.locked.compare_exchange_weak(
            false,
            true,
            core::sync::atomic::Ordering::Acquire,
            core::sync::atomic::Ordering::Relaxed,
        ).is_err() {
            while self.locked.load(core::sync::atomic::Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
    }

    /// Releases the lock.
    #[inline(always)]
    pub fn unlock(&self) {
        self.locked.store(false, core::sync::atomic::Ordering::Release);
    }
}

