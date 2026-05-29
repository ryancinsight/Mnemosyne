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

#[cfg(target_pointer_width = "64")]
impl AtomicFreeList {
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
    pub fn push(&self, block: NonNull<Block>) {
        let block_ptr = block.as_ptr();
        let block_usize = block_ptr as usize;
        // Ensure the pointer fits in 48 bits (bits 48-63 must be 0)
        debug_assert_eq!(
            block_usize & 0xFFFF_0000_0000_0000,
            0,
            "Pointer uses upper 16 bits: {:p}",
            block_ptr
        );

        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            // Unpack current head pointer
            let current_ptr = (current & 0x0000_FFFF_FFFF_FFFF) as *mut Block;
            // Unpack current count and increment
            let current_count = current >> 48;
            let next_count = (current_count + 1) & 0xFFFF;

            // Safety: block_ptr is guaranteed to be valid, writeable, aligned memory,
            // exclusive to the thread calling push.
            unsafe {
                (*block_ptr).next = NonNull::new(current_ptr);
            }

            let next_val = (next_count << 48) | block_usize;

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
    pub fn pop_all(&self) -> Option<(NonNull<Block>, usize)> {
        let val = self.head.swap(0, Ordering::Acquire);
        if val == 0 {
            None
        } else {
            let ptr_val = val & 0x0000_FFFF_FFFF_FFFF;
            let count = val >> 48;
            NonNull::new(ptr_val as *mut Block).map(|head| (head, count))
        }
    }

    /// Checks if the atomic list is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        (self.head.load(Ordering::Relaxed) & 0x0000_FFFF_FFFF_FFFF) == 0
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
    pub fn push(&self, block: NonNull<Block>) {
        let block_ptr = block.as_ptr();
        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            // Safety: block_ptr is guaranteed to be valid, writeable, aligned memory,
            // exclusive to the thread calling push.
            unsafe {
                (*block_ptr).next = NonNull::new(current);
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
    pub fn pop_all(&self) -> Option<(NonNull<Block>, usize)> {
        let ptr = self.head.swap(core::ptr::null_mut(), Ordering::Acquire);
        NonNull::new(ptr).map(|head| {
            let mut count = 0;
            let mut current = Some(head);
            while let Some(node) = current {
                count += 1;
                current = unsafe { (*node.as_ptr()).next };
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
