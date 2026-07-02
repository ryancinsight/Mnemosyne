use core::alloc::Layout;
use core::ptr::NonNull;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::internal::{
    allocate_large_or_huge, deallocate_large_or_huge, do_local_free_internal,
    ensure_options_initialized, initialize_allocated_bytes, is_valid_layout_alloc_request,
    poison_freed_bytes, size_to_class_nonzero, Block, HasSegmentPool, Segment, ThreadAllocator,
    MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, PAGES_PER_SEGMENT, PAGE_SHIFT, SEGMENT_SIZE,
};

pub(crate) struct RawHeap<P: AllocPolicy, B: HasSegmentPool> {
    allocator: core::cell::UnsafeCell<ThreadAllocator<B>>,
    _policy: core::marker::PhantomData<P>,
}

// SAFETY: `RawHeap<P, B>` holds a single `UnsafeCell<ThreadAllocator<B>>`
// and a ZST `PhantomData<P>`. `ThreadAllocator` is the per-thread
// allocator state (free lists, page lists, current segment); the
// `UnsafeCell` is what lets `&self` methods take the `&mut
// ThreadAllocator` the allocation/free paths need. `RawHeap` is not
// auto-`Send` only because `UnsafeCell<T>: !Sync` denies the auto-derive,
// not because concurrent access is sound — these methods assume exclusive
// access to the allocator and perform no internal synchronization.
//
// Cross-thread *transfer* is nonetheless sound, and required, because the
// branded wrapper `Heap<'brand, P, B>` is the only constructor and it
// confines the heap to one thread at runtime: the `'brand` invariant
// lifetime is minted exclusively through `thread_local_scope`, whose
// `ThreadLocalToken` is `!Send + !Sync`, so the heap and its token stay on
// the spawning thread for the scope's lifetime. `Send` is the necessary
// trait surface so the heap can move between threads in pathological call
// patterns; the brand mint, not this impl, is what precludes two threads
// touching the same `ThreadAllocator` concurrently. This mirrors the
// `unsafe impl Send for TieredHeap` reasoning in `tiered_heap.rs`.
unsafe impl<P: AllocPolicy, B: HasSegmentPool> Send for RawHeap<P, B> {}

impl<P: AllocPolicy, B: HasSegmentPool> Default for RawHeap<P, B> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<P: AllocPolicy, B: HasSegmentPool> RawHeap<P, B> {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self {
            allocator: core::cell::UnsafeCell::new(ThreadAllocator::new()),
            _policy: core::marker::PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `alloc_inner` only requires that no other borrow of the
        // `UnsafeCell<ThreadAllocator>` is live across the call; this `&self`
        // method holds no such borrow, and thread-confinement (see the
        // `unsafe impl Send`) guarantees no concurrent accessor on another
        // thread.
        let ptr = unsafe { self.alloc_inner(layout) };
        if mnemosyne_prof::is_active() && !ptr.is_null() {
            mnemosyne_prof::on_alloc(ptr, layout.size());
        }
        ptr
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn stats(&self) -> mnemosyne_local::ThreadAllocatorStats {
        // SAFETY: forms a shared `&ThreadAllocator` to read immutable
        // statistics. No `&mut` borrow of the `UnsafeCell` is live in this
        // test-only accessor, and thread-confinement precludes a concurrent
        // mutator, so the shared reference does not alias a live exclusive one.
        unsafe { (&*self.allocator.get()).stats() }
    }

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
    unsafe fn alloc_inner(&self, layout: Layout) -> *mut u8 {
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

        // SAFETY: by this function's `# Safety` contract the
        // `UnsafeCell<ThreadAllocator>` is exclusively borrowable here (no
        // other live borrow, thread-confined), so forming `&mut` is sound.
        let alloc = unsafe { &mut *self.allocator.get() };
        if alloc.is_allocating {
            // SAFETY: re-entrant alloc (the allocator is mid-operation);
            // serve from the large/huge path with the validated
            // `adjusted_size`/`align`, avoiding re-borrowing the small path.
            return unsafe { Self::alloc_large_or_huge_init(adjusted_size, align) };
        }

        alloc.is_allocating = true;
        // SAFETY: `class` is a valid small size class produced by
        // `size_to_class_nonzero`; `alloc` is the exclusively-borrowed
        // allocator, and the `is_allocating` flag set above guards against
        // re-entrant small-path use during this call.
        let ptr = unsafe { alloc.alloc_class::<P>(class) };
        alloc.is_allocating = false;

        if !ptr.is_null() {
            // SAFETY: `ptr` is the non-null block just returned by
            // `alloc_class` for `class`, whose block size is at least
            // `adjusted_size`; initializing `adjusted_size` bytes stays within
            // it.
            unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
        }
        ptr
    }

    /// # Safety
    ///
    /// `ptr` must be null or a live block previously returned by this
    /// `RawHeap`'s `alloc`, not yet freed. The allocator must be
    /// exclusively accessible (no concurrent access; brand-confined to one
    /// thread).
    #[inline(always)]
    pub(crate) unsafe fn free_owned_unchecked(&self, ptr: *mut u8) {
        ensure_options_initialized();
        if ptr.is_null() {
            return;
        }

        let ptr_val = ptr as usize;
        let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
        // SAFETY: `ptr` was returned by this allocator, so masking off the
        // low `SEGMENT_SIZE` bits recovers the live segment header that was
        // initialized at allocation time. `page_index` is bounded by the
        // `(PAGES_PER_SEGMENT - 1)` mask, so it indexes the `pages` array.
        let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };

        if mnemosyne_prof::is_active() {
            // SAFETY: `ptr`, `page`, and `page_index` are the live triple just
            // derived from a valid allocation; `allocation_size` only reads
            // metadata inside the originating segment/mapping.
            let size = unsafe { allocation_size(ptr, page, page_index) };
            mnemosyne_prof::on_free(ptr, size);
        }

        if page_index == 0 || page.block_size == 0 {
            // SAFETY: `page_index == 0` or a zero `block_size` identifies a
            // large/huge allocation, whose deallocation path `ptr` was routed
            // through at alloc time; the `# Safety` contract guarantees `ptr`
            // is live and owned.
            unsafe { free_large_or_huge::<P, B>(ptr) };
            return;
        }

        if P::ENABLE_POISONING {
            // SAFETY: small-page free — `page.block_size` is the exact block
            // stride of `page`, and `ptr` is a live block of that page, so
            // poisoning `block_size` bytes stays within the block.
            unsafe { poison_freed_bytes::<P>(ptr, page.block_size) };
        }

        // SAFETY: `ptr` is a small-page block (`page_index != 0`,
        // `block_size != 0`) of the recovered `page`/`segment` at
        // `page_index`; `free_owned` consumes the matching block/page/segment
        // triple under the heap's exclusive access.
        unsafe { self.free_owned(ptr as *mut Block, page, segment, page_index) };
    }

    /// # Safety
    ///
    /// `block` must be a live block of `page`, which must be `page_index` of
    /// the live `segment`; the three must be the matching triple recovered
    /// from one allocation. The allocator must be exclusively accessible.
    #[inline(always)]
    unsafe fn free_owned(
        &self,
        block: *mut Block,
        page: &mut mnemosyne_core::types::Page,
        segment: *mut Segment,
        page_index: usize,
    ) {
        // SAFETY: exclusive, thread-confined access to the allocator per the
        // `# Safety` contract, so `&mut` from the `UnsafeCell` is sound.
        let alloc = unsafe { &mut *self.allocator.get() };
        if alloc.is_allocating {
            // SAFETY: re-entrant free while the allocator is mid-operation;
            // `block` is a non-null live block of `page` (allocator
            // invariant), so `new_unchecked` is sound and the page-local
            // atomic free list takes ownership of it.
            unsafe {
                page.thread_free.push::<P>(NonNull::new_unchecked(block));
            }
            return;
        }

        let page_free = page.free;
        let page_alloc_count = page.alloc_count;
        // SAFETY: `segment` is the live segment owning `page`; `page_index`
        // indexes its `keys` array (sized `PAGES_PER_SEGMENT`) and is the
        // page's own index, so the read is in bounds.
        let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
            unsafe { (*segment).keys[page_index] }
        } else {
            0
        };
        // SAFETY: `(*segment).is_current` reads a flag in the live segment
        // header.
        if page_alloc_count != 1 || unsafe { (*segment).is_current } {
            // SAFETY: `block` is a live non-null block of `page` (allocator
            // invariant); writing its `next` link, publishing it as the
            // free-list head, and decrementing the page/segment occupancy all
            // stay within `page`/`segment` under exclusive access.
            unsafe {
                (*block).set_next::<P>(page_free, cookie);
                page.free = Some(NonNull::new_unchecked(block));
                page.decrement_alloc_count_for_segment(segment, page_index);
            }
            return;
        }

        alloc.is_allocating = true;
        // SAFETY: the matching `block`/`page`/`segment`/`page_index` triple
        // from the `# Safety` contract is passed to the internal free, which
        // runs under the exclusive `alloc` borrow with the `is_allocating`
        // guard set.
        let became_empty =
            unsafe { do_local_free_internal::<P, B>(alloc, block, page, segment, page_index) };

        if became_empty {
            // SAFETY: `alloc` is the exclusively-borrowed allocator; recording
            // a defrag operation only mutates its own bookkeeping.
            unsafe { alloc.record_defrag_operation::<P>() };
        }

        alloc.is_allocating = false;
    }

    /// # Safety
    ///
    /// `ptr` must be null or a live block previously returned by this
    /// `RawHeap` under `layout`, not yet freed; `layout` must be the layout
    /// it was allocated with. The allocator must be exclusively accessible
    /// (brand-confined to one thread).
    #[inline(always)]
    pub(crate) unsafe fn realloc_owned_unchecked(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        ensure_options_initialized();
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
            return self.alloc(Layout::from_size_align(new_size, layout.align()).unwrap_or(layout));
        }

        // SAFETY: `ptr` is a live block allocated under `layout` per the
        // `# Safety` contract; `can_reuse_allocation` only reads metadata of
        // the existing allocation.
        if unsafe { self.can_reuse_allocation(ptr, layout, new_size) } {
            return ptr;
        }

        let new_ptr =
            self.alloc(Layout::from_size_align(new_size, layout.align()).unwrap_or(layout));
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

/// # Safety
///
/// `ptr` must be a live large/huge block previously returned by this
/// backend `B`, not yet freed; the segment pointer is stored in the slot
/// immediately preceding the user payload at allocation time.
#[cold]
#[inline(never)]
unsafe fn free_large_or_huge<P: AllocPolicy, B: HasSegmentPool>(ptr: *mut u8) {
    // SAFETY: every large/huge allocation writes its owning `*mut Segment`
    // into the pointer-sized slot directly preceding the user payload, so
    // `(ptr as *mut *mut Segment) - 1` reads back that live segment pointer.
    let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
    if P::ENABLE_POISONING {
        // SAFETY: `ptr`/`segment` are the live block and its owning segment;
        // `huge_or_large_size` reads only metadata inside that mapping.
        let size = unsafe { huge_or_large_size(ptr, segment) };
        // SAFETY: `size` is the block's allocated length, so poisoning that
        // many bytes from `ptr` stays within the mapping.
        unsafe { poison_freed_bytes::<P>(ptr, size) };
    }
    // SAFETY: `ptr` and its recovered owning `segment` form the matching pair
    // for `deallocate_large_or_huge`, which releases the originating mapping.
    let _released = unsafe { deallocate_large_or_huge::<B>(ptr, segment) };
}

/// # Safety
///
/// `ptr` must be a live block of this allocator and `page`/`page_index`
/// the segment page recovered from it, as in `free_owned_unchecked`.
#[inline(always)]
unsafe fn allocation_size(
    ptr: *mut u8,
    page: &mnemosyne_core::types::Page,
    page_index: usize,
) -> usize {
    if page_index == 0 || page.block_size == 0 {
        // SAFETY: large/huge classification — the owning `*mut Segment` lives
        // in the slot directly preceding the user payload (written at alloc
        // time), so this read recovers the live segment pointer.
        let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        // SAFETY: `ptr`/`segment` are the live block and its owning segment;
        // `huge_or_large_size` reads only metadata inside that mapping.
        unsafe { huge_or_large_size(ptr, segment) }
    } else {
        page.block_size
    }
}

/// # Safety
///
/// `segment` must be the live segment owning `ptr`'s large/huge mapping
/// (as recovered from the metadata slot preceding the payload).
#[inline(always)]
unsafe fn huge_or_large_size(ptr: *mut u8, segment: *mut Segment) -> usize {
    // SAFETY: `segment` is the live owning segment; `pages[0].alloc_count`
    // holds the large allocation's byte length (repurposed for large blocks).
    let size = unsafe { (*segment).pages[0].alloc_count };
    if size > 0 {
        size
    } else {
        // SAFETY: a zero `pages[0].alloc_count` marks a huge mapping, whose
        // length is derived from `ptr`'s offset within `segment`'s mapping.
        unsafe { (*segment).huge_mapping_suffix_from(ptr) }
    }
}
