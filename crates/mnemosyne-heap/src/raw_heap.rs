use core::alloc::Layout;
use core::ptr::NonNull;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::internal::{
    allocate_large_or_huge, deallocate_large_or_huge, do_local_free_internal,
    ensure_options_initialized, initialize_allocated_bytes, is_valid_layout_alloc_request,
    poison_freed_bytes, size_to_class_nonzero, Block, HasSegmentPool, Segment,
    ThreadAllocator, MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, PAGES_PER_SEGMENT, PAGE_SHIFT,
    SEGMENT_SIZE,
};

pub(crate) struct RawHeap<P: AllocPolicy, B: HasSegmentPool> {
    allocator: core::cell::UnsafeCell<ThreadAllocator<B>>,
    _policy: core::marker::PhantomData<P>,
}

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
        let ptr = unsafe { self.alloc_inner(layout) };
        if mnemosyne_prof::is_active() && !ptr.is_null() {
            mnemosyne_prof::on_alloc(ptr, layout.size());
        }
        ptr
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn stats(&self) -> mnemosyne_local::ThreadAllocatorStats {
        unsafe { (&*self.allocator.get()).stats() }
    }

    #[inline(always)]
    unsafe fn alloc_inner(&self, layout: Layout) -> *mut u8 {
        ensure_options_initialized();
        if !is_valid_layout_alloc_request(layout.size(), layout.align()) {
            return core::ptr::null_mut();
        }

        let size = layout.size();
        let align = layout.align();
        if align > MIN_BLOCK_SIZE {
            let ptr = unsafe { allocate_large_or_huge::<B>(size, align, true) };
            if !ptr.is_null() {
                unsafe { initialize_allocated_bytes::<P>(ptr, size) };
            }
            return ptr;
        }

        let adjusted_size = core::cmp::max(size, align);
        let class = match size_to_class_nonzero(adjusted_size) {
            Some(c) => c,
            None => {
                let ptr = unsafe {
                    allocate_large_or_huge::<B>(adjusted_size, align, true)
                };
                if !ptr.is_null() {
                    unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                }
                return ptr;
            }
        };

        let alloc = unsafe { &mut *self.allocator.get() };
        if alloc.is_allocating {
            let ptr = unsafe { allocate_large_or_huge::<B>(adjusted_size, align, true) };
            if !ptr.is_null() {
                unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
            }
            return ptr;
        }

        alloc.is_allocating = true;
        let ptr = unsafe { alloc.alloc_class::<P>(class) };
        alloc.is_allocating = false;

        if !ptr.is_null() {
            unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
        }
        ptr
    }

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
        let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };

        if mnemosyne_prof::is_active() {
            let size = unsafe { allocation_size(ptr, page, page_index) };
            mnemosyne_prof::on_free(ptr, size);
        }

        if page_index == 0 || page.block_size == 0 {
            unsafe { free_large_or_huge::<P, B>(ptr) };
            return;
        }

        if P::ENABLE_POISONING {
            unsafe { poison_freed_bytes::<P>(ptr, page.block_size) };
        }

        unsafe { self.free_owned(ptr as *mut Block, page, segment, page_index) };
    }

    #[inline(always)]
    unsafe fn free_owned(
        &self,
        block: *mut Block,
        page: &mut mnemosyne_core::types::Page,
        segment: *mut Segment,
        page_index: usize,
    ) {
        let alloc = unsafe { &mut *self.allocator.get() };
        if alloc.is_allocating {
            unsafe {
                page.thread_free.push::<P>(NonNull::new_unchecked(block));
            }
            return;
        }

        let page_free = page.free;
        let page_alloc_count = page.alloc_count;
        let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
            unsafe { (*segment).keys[page_index] }
        } else {
            0
        };
        if page_alloc_count != 1 || unsafe { (*segment).is_current } {
            unsafe {
                (*block).set_next::<P>(page_free, cookie);
                page.free = Some(NonNull::new_unchecked(block));
                page.decrement_alloc_count_for_segment(segment, page_index);
            }
            return;
        }

        alloc.is_allocating = true;
        let became_empty =
            unsafe { do_local_free_internal::<P, B>(alloc, block, page, segment, page_index) };

        if became_empty {
            unsafe { alloc.record_defrag_operation::<P>() };
        }

        alloc.is_allocating = false;
    }

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
                unsafe { self.free_owned_unchecked(ptr) };
            }
            return core::ptr::null_mut();
        }
        if ptr.is_null() {
            return self.alloc(Layout::from_size_align(new_size, layout.align()).unwrap_or(layout));
        }

        if unsafe { self.can_reuse_allocation(ptr, layout, new_size) } {
            return ptr;
        }

        let new_ptr =
            self.alloc(Layout::from_size_align(new_size, layout.align()).unwrap_or(layout));
        if new_ptr.is_null() {
            return core::ptr::null_mut();
        }

        unsafe {
            core::ptr::copy_nonoverlapping(ptr, new_ptr, core::cmp::min(layout.size(), new_size));
            self.free_owned_unchecked(ptr);
        }
        new_ptr
    }

    #[inline(always)]
    unsafe fn can_reuse_allocation(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> bool {
        if P::ZERO_INITIALIZE || P::ENABLE_POISONING {
            return false;
        }
        if new_size <= layout.size() {
            if layout.size() <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                return new_size >= layout.size() / 2;
            }

            let current_usable = unsafe { mnemosyne_local::usable_size(ptr) };
            let new_adjusted = core::cmp::max(new_size, layout.align());
            if new_adjusted <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                return new_size >= layout.size() / 2;
            }

            let page_size = mnemosyne_core::constants::PAGE_SIZE;
            let new_page_rounded = (new_adjusted + page_size - 1) & !(page_size - 1);
            return new_page_rounded >= current_usable;
        }

        if layout.size() <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
            return mnemosyne_local::internal::small_realloc_fits_existing_class(layout, new_size);
        }

        let current_usable = unsafe { mnemosyne_local::usable_size(ptr) };
        new_size <= current_usable
    }
}

#[cold]
#[inline(never)]
unsafe fn free_large_or_huge<P: AllocPolicy, B: HasSegmentPool>(ptr: *mut u8) {
    let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
    if P::ENABLE_POISONING {
        let size = unsafe { huge_or_large_size(ptr, segment) };
        unsafe { poison_freed_bytes::<P>(ptr, size) };
    }
    let _released = unsafe { deallocate_large_or_huge::<B>(ptr, segment) };
}

#[inline(always)]
unsafe fn allocation_size(
    ptr: *mut u8,
    page: &mnemosyne_core::types::Page,
    page_index: usize,
) -> usize {
    if page_index == 0 || page.block_size == 0 {
        let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        unsafe { huge_or_large_size(ptr, segment) }
    } else {
        page.block_size
    }
}

#[inline(always)]
unsafe fn huge_or_large_size(ptr: *mut u8, segment: *mut Segment) -> usize {
    let size = unsafe { (*segment).pages[0].alloc_count };
    if size > 0 {
        size
    } else {
        unsafe { (*segment).huge_mapping_suffix_from(ptr) }
    }
}
