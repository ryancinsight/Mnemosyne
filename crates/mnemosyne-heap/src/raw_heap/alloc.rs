//! Allocation path for [`RawHeap`]: the small-class fast route, the
//! large/huge fallback, and the re-entrancy-aware `alloc_inner` dispatch.

use super::RawHeap;
use core::alloc::Layout;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::LocalAllocatorSelector;
use mnemosyne_local::internal::{
    HasSegmentPool, MIN_BLOCK_SIZE, allocate_large_or_huge, ensure_options_initialized,
    initialize_allocated_bytes, is_valid_layout_alloc_request, size_to_class_nonzero,
};

impl<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> RawHeap<P, B> {
    /// Allocates through the large/huge path and applies the policy's
    /// byte-initialization to the fresh block — the single shared tail for
    /// the three `alloc_inner` branches that bypass the small-class fast
    /// path (over-aligned, over-sized, and re-entrant requests).
    ///
    /// # Safety
    ///
    /// `size`/`align` must form a valid allocation request already vetted by
    /// `is_valid_layout_alloc_request` (with `size` possibly adjusted upward
    /// to `max(size, align)`, which preserves validity for the large/huge
    /// path).
    #[inline]
    unsafe fn alloc_large_or_huge_init(size: usize, align: usize) -> *mut u8 {
        // SAFETY: by this function's contract `size`/`align` are a vetted
        // large/huge allocation request.
        let ptr = unsafe { allocate_large_or_huge::<B>(size, align, true) };
        if !ptr.is_null() {
            // SAFETY: `ptr` is the non-null `size`-byte block just returned
            // by `allocate_large_or_huge`; initializing exactly `size` bytes
            // stays within it.
            unsafe { initialize_allocated_bytes::<P>(ptr, size) };
        }
        ptr
    }

    /// # Safety
    ///
    /// No other borrow of the `UnsafeCell<ThreadAllocator>` may be live
    /// across this call (the allocator is exclusively borrowed here), and
    /// no other thread may access this `RawHeap` concurrently — both
    /// guaranteed by the brand-based thread-confinement on `Heap`.
    #[inline(always)]
    pub(super) unsafe fn alloc_inner(&self, layout: Layout) -> *mut u8 {
        ensure_options_initialized();
        if !is_valid_layout_alloc_request(layout.size(), layout.align()) {
            return core::ptr::null_mut();
        }

        let size = layout.size();
        let align = layout.align();
        if align > MIN_BLOCK_SIZE {
            // SAFETY: `size`/`align` passed `is_valid_layout_alloc_request`
            // above, so they are a valid allocation request for the
            // large/huge path (over-aligned small allocations route here).
            return unsafe { Self::alloc_large_or_huge_init(size, align) };
        }

        let adjusted_size = core::cmp::max(size, align);
        let class = match size_to_class_nonzero(adjusted_size) {
            Some(c) => c,
            None => {
                // SAFETY: `adjusted_size = max(size, align)` exceeds the
                // largest small size class (`size_to_class_nonzero` returned
                // `None`), so it is a valid large/huge request; `align` was
                // validated above.
                return unsafe { Self::alloc_large_or_huge_init(adjusted_size, align) };
            }
        };

        // Gate before borrowing: reading it through a `&mut` formed here would
        // be the aliasing the gate exists to reject.
        if self.is_allocating.get() {
            // SAFETY: re-entrant alloc (the allocator is mid-operation);
            // serve from the large/huge path with the validated
            // `adjusted_size`/`align`, avoiding re-borrowing the small path.
            return unsafe { Self::alloc_large_or_huge_init(adjusted_size, align) };
        }

        self.is_allocating.set(true);
        // SAFETY: by this function's `# Safety` contract and the gate above the
        // `UnsafeCell<ThreadAllocator>` is exclusively borrowable here.
        let alloc = unsafe { &mut *self.allocator.get() };
        // SAFETY: `class` is a valid small size class produced by
        // `size_to_class_nonzero`; `alloc` is the exclusively-borrowed
        // allocator, and the `is_allocating` flag set above guards against
        // re-entrant small-path use during this call.
        let ptr = unsafe { alloc.alloc_class::<P>(class) };
        self.is_allocating.set(false);

        if !ptr.is_null() {
            // SAFETY: `ptr` is the non-null block just returned by
            // `alloc_class` for `class`, whose block size is at least
            // `adjusted_size`; initializing `adjusted_size` bytes stays within
            // it.
            unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
        }
        ptr
    }
}
