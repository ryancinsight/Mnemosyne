//! Small-class allocation hot path for [`super::ThreadAllocator`].
//!
//! [`ThreadAllocator::alloc_class`] is the fast dispatch that serves the
//! common case (active page has a free block) inline and falls through to the
//! outlined cold path in [`cold`] only when all active pages are exhausted.

mod cold;

use super::page::{try_allocate_page_local, try_reclaim_and_allocate};
use crate::local_alloc::ThreadAllocator;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::policy::AllocPolicy;
#[cfg(test)]
use mnemosyne_core::size_class::size_to_class;

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Allocates a small memory block of the specified size class.
    ///
    /// # Safety
    ///
    /// `class` must be a valid size class index (< `NUM_SIZE_CLASSES`). Every
    /// policy used with this allocator instance must have the same
    /// `ENABLE_FREE_LIST_ENCRYPTION` value; the public `thread_*` entry points
    /// enforce that separation through their mode-keyed TLS slots.
    #[inline(always)]
    pub unsafe fn alloc_class<P: AllocPolicy>(&mut self, class: usize) -> *mut u8 {
        if let Some(page_ptr) = unsafe { *self.active_pages.get_unchecked(class) } {
            // Raw pointer, not `&mut`: these paths reach the parent segment, and
            // a `Unique` tag minted here would have to be popped by that access.
            let page = page_ptr.as_ptr();

            // 1. Check thread-local free list or lazy bump allocation.
            if let Some(block) = unsafe { try_allocate_page_local::<P>(page) } {
                return block.as_ptr() as *mut u8;
            }

            // 2. Reclaim batched cross-thread frees only after the local list is empty.
            // SAFETY: `page` is owned by this allocator and `try_reclaim_and_allocate`
            // upholds the `Page::reclaim_thread_free` contract on its behalf.
            if let Some(block) =
                unsafe { try_reclaim_and_allocate::<P>(page, &mut self.cross_thread_reclaimed) }
            {
                return block.as_ptr() as *mut u8;
            }
        }

        // Outline the cold allocation path to keep alloc() small and fast.
        // SAFETY: `class` is the same caller-validated size-class index
        // (< `NUM_SIZE_CLASSES`, the contract of `alloc_class`) that indexed
        // `active_pages` above, satisfying `alloc_cold`'s bounds precondition.
        unsafe { self.alloc_cold::<P>(class) }
    }

    /// Allocates a block of memory of the given size.
    ///
    /// Returns null if the size is not a small class or if allocation fails.
    ///
    /// This size-taking entry point exists only for the crate's own tests, which
    /// drive the allocator by byte size; production callers route through
    /// `alloc_class` (size-class already resolved) or the crate's public
    /// `thread_alloc*` entry points, so it is gated out of non-test builds to
    /// keep the public unsafe surface minimal.
    ///
    /// # Safety
    ///
    /// This method is unsafe because it works with raw pointers and handles
    /// manual memory layouts. The policy mode must match the mode used by all
    /// pages already owned by this allocator instance.
    #[cfg(test)]
    #[inline(always)]
    pub unsafe fn alloc<P: AllocPolicy>(&mut self, size: usize) -> *mut u8 {
        let class = match size_to_class(size) {
            Some(c) => c,
            None => return core::ptr::null_mut(),
        };
        unsafe { self.alloc_class::<P>(class) }
    }
}
