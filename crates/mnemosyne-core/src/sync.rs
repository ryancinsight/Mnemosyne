//! Synchronization primitives for the allocator, including lock-free structures.

use crate::types::Block;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicPtr, Ordering};

/// A lock-free, atomic singly-linked list of blocks.
///
/// Implements atomic push and atomic pop-all operations, matching the deallocation
/// queue pattern from mimalloc.
pub struct AtomicFreeList {
    head: AtomicPtr<Block>,
}

impl AtomicFreeList {
    /// Creates a new empty `AtomicFreeList`.
    pub const fn new() -> Self {
        Self {
            head: AtomicPtr::new(core::ptr::null_mut()),
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

    /// Atomically removes all blocks from the list and returns the head.
    ///
    /// This is wait-free and returns a standard local linked list.
    #[inline]
    pub fn pop_all(&self) -> Option<NonNull<Block>> {
        let ptr = self.head.swap(core::ptr::null_mut(), Ordering::Acquire);
        NonNull::new(ptr)
    }

    /// Checks if the atomic list is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Relaxed).is_null()
    }
}
