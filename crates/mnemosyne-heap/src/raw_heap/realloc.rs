//! Reallocation path for [`RawHeap`]: the grow/shrink dispatch and the
//! in-place `can_reuse_allocation` decision that avoids a copy when the
//! existing block already satisfies the new layout.

use super::RawHeap;
use core::alloc::Layout;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::LocalAllocatorSelector;
use mnemosyne_local::internal::{
    HasSegmentPool, MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, ensure_options_initialized,
};

impl<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> RawHeap<P, B> {
    /// # Safety
    ///
    /// `ptr` must be null or a live block previously returned by this
    /// `RawHeap` under `layout`, not yet freed; `layout` must be the layout
    /// it was allocated with. `new_layout` must be a valid layout for the
    /// requested replacement block. The allocator must be exclusively
    /// accessible (brand-confined to one thread).
    #[inline(always)]
    pub(crate) unsafe fn realloc_owned_unchecked(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_layout: Layout,
    ) -> *mut u8 {
        ensure_options_initialized();
        let new_size = new_layout.size();
        if new_size == 0 {
            if !ptr.is_null() {
                // SAFETY: zero-realloc frees the block. `ptr` is a live block
                // of this heap per the `# Safety` contract, satisfying
                // `free_owned_unchecked`.
                unsafe { self.free_owned_unchecked(ptr) };
            }
            return core::ptr::null_mut();
        }
        if ptr.is_null() {
            return self.alloc(new_layout);
        }

        // SAFETY: `ptr` is a live block allocated under `layout` per the
        // `# Safety` contract; `can_reuse_allocation` only reads metadata of
        // the existing allocation.
        if unsafe { self.can_reuse_allocation(ptr, layout, new_size) } {
            return ptr;
        }

        let new_ptr = self.alloc(new_layout);
        if new_ptr.is_null() {
            return core::ptr::null_mut();
        }

        // SAFETY: `ptr` is the live old block of at least `layout.size()`
        // bytes and `new_ptr` is the freshly allocated block of at least
        // `new_size` bytes; the two allocations are distinct so the copy of
        // `min(layout.size(), new_size)` bytes is non-overlapping and in
        // bounds of both. The old block is then freed (still live and owned).
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, new_ptr, core::cmp::min(layout.size(), new_size));
            self.free_owned_unchecked(ptr);
        }
        new_ptr
    }

    /// # Safety
    ///
    /// `ptr` must be a live block previously returned by this `RawHeap` under
    /// `layout` — `usable_size(ptr)` reads the originating segment metadata.
    #[inline(always)]
    unsafe fn can_reuse_allocation(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> bool {
        if P::ZERO_INITIALIZE || P::ENABLE_POISONING {
            return false;
        }
        if new_size <= layout.size() {
            if layout.size() <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                return new_size >= layout.size() / 2;
            }

            let new_adjusted = core::cmp::max(new_size, layout.align());
            if new_adjusted <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                return new_size >= layout.size() / 2;
            }

            // The segment-header read happens only on the branch that
            // consumes it: the small-class shrink decisions above never use
            // the block's usable size, so hoisting this load ahead of them
            // would put a dead metadata read on the hot realloc path.
            // SAFETY: `ptr` is a live block of this heap per the `# Safety`
            // contract; `usable_size` reads the originating segment/page
            // metadata to recover the block's true capacity.
            let current_usable = unsafe { mnemosyne_local::usable_size(ptr) };
            let page_size = mnemosyne_core::constants::PAGE_SIZE;
            let new_page_rounded = (new_adjusted + page_size - 1) & !(page_size - 1);
            return new_page_rounded >= current_usable;
        }

        if layout.size() <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
            return mnemosyne_local::internal::small_realloc_fits_existing_class(layout, new_size);
        }

        // SAFETY: `ptr` is a live block of this heap per the `# Safety`
        // contract; `usable_size` reads the originating segment/page metadata
        // to recover the block's true capacity for the large/over-aligned case.
        let current_usable = unsafe { mnemosyne_local::usable_size(ptr) };
        new_size <= current_usable
    }
}
