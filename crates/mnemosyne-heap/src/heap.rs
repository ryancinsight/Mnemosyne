use core::alloc::Layout;
use core::ptr::NonNull;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::internal::{
    allocate_large_or_huge, deallocate_large_or_huge, do_local_free_internal,
    ensure_options_initialized, initialize_allocated_bytes, is_valid_layout_alloc_request,
    poison_freed_bytes, size_to_class_nonzero, Block, HasSegmentPool, Segment, ThreadAllocator,
    MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, PAGES_PER_SEGMENT, PAGE_SHIFT, SEGMENT_SIZE,
};

/// An explicit custom memory heap.
///
/// Threads can instantiate a `MnemosyneHeap` to manage their own isolated allocation stream.
/// When the heap is dropped, all segments owned by it are automatically reclaimed or orphaned.
pub struct MnemosyneHeap<P: AllocPolicy, B: HasSegmentPool = mnemosyne_backend::DefaultBackend> {
    allocator: core::cell::UnsafeCell<ThreadAllocator<B>>,
    _phantom: core::marker::PhantomData<P>,
}

unsafe impl<P: AllocPolicy, B: HasSegmentPool> Send for MnemosyneHeap<P, B> {}

impl<P: AllocPolicy, B: HasSegmentPool> Default for MnemosyneHeap<P, B> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<P: AllocPolicy, B: HasSegmentPool> MnemosyneHeap<P, B> {
    /// Creates a new empty `MnemosyneHeap`.
    pub const fn new() -> Self {
        Self {
            allocator: core::cell::UnsafeCell::new(ThreadAllocator::new()),
            _phantom: core::marker::PhantomData,
        }
    }

    /// Allocates a block of memory from this heap.
    ///
    /// Returns null if allocation fails.
    #[inline(always)]
    pub fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.alloc_inner(layout) };
        if mnemosyne_prof::is_active() && !ptr.is_null() {
            mnemosyne_prof::on_alloc(ptr, layout.size());
        }
        ptr
    }

    #[inline(always)]
    unsafe fn alloc_inner(&self, layout: Layout) -> *mut u8 {
        ensure_options_initialized();
        if !is_valid_layout_alloc_request(layout.size(), layout.align()) {
            return core::ptr::null_mut();
        }
        let alloc = unsafe { &mut *self.allocator.get() };
        if alloc.is_allocating {
            // Re-entrancy protection fallback.
            return unsafe {
                allocate_large_or_huge::<B>(layout.size(), layout.align(), P::ENABLE_POISONING)
            };
        }

        let size = layout.size();
        let align = layout.align();
        if align > MIN_BLOCK_SIZE {
            let ptr = unsafe { allocate_large_or_huge::<B>(size, align, P::ENABLE_POISONING) };
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
                    allocate_large_or_huge::<B>(adjusted_size, align, P::ENABLE_POISONING)
                };
                if !ptr.is_null() {
                    unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                }
                return ptr;
            }
        };

        alloc.is_allocating = true;
        let ptr = unsafe { alloc.alloc_class::<P>(class) };
        let final_ptr = if ptr.is_null() {
            unsafe { allocate_large_or_huge::<B>(adjusted_size, align, P::ENABLE_POISONING) }
        } else {
            ptr
        };
        if !final_ptr.is_null() {
            unsafe { initialize_allocated_bytes::<P>(final_ptr, adjusted_size) };
        }
        alloc.is_allocating = false;
        final_ptr
    }

    /// Frees a block of memory back to its originating heap/allocator.
    ///
    /// # Safety
    ///
    /// The pointer must be non-null and previously allocated by this heap.
    #[inline(always)]
    pub unsafe fn free(&self, ptr: *mut u8) {
        ensure_options_initialized();
        if ptr.is_null() {
            return;
        }

        let ptr_val = ptr as usize;
        let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

        // Safety: ptr is non-null and comes from Mnemosyne.
        let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };
        if mnemosyne_prof::is_active() {
            let size = if page_index == 0 || page.block_size == 0 {
                let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
                let size = unsafe { (*segment).pages[0].alloc_count };
                if size > 0 {
                    size
                } else {
                    unsafe { (*segment).huge_mapping_suffix_from(ptr) }
                }
            } else {
                page.block_size
            };
            mnemosyne_prof::on_free(ptr, size);
        }

        if page_index == 0 || page.block_size == 0 {
            if P::ENABLE_POISONING {
                let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
                let size = unsafe { (*segment).pages[0].alloc_count };
                let size = if size > 0 {
                    size
                } else {
                    unsafe { (*segment).huge_mapping_suffix_from(ptr) }
                };
                unsafe { poison_freed_bytes::<P>(ptr, size) };
            }
            let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            let _released = unsafe { deallocate_large_or_huge::<B>(ptr, segment) };
            return;
        }

        if P::ENABLE_POISONING {
            unsafe { poison_freed_bytes::<P>(ptr, page.block_size) };
        }

        let block = ptr as *mut Block;
        let owner = unsafe { (*segment).owner };
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        let heap_ptr = self.allocator.get() as *mut ThreadAllocator<B> as *mut core::ffi::c_void;

        #[cfg(all(windows, target_arch = "x86_64"))]
        let is_owner = {
            let tid = unsafe {
                let val: u32;
                core::arch::asm!(
                    "mov {0:e}, gs:[0x48]",
                    out(reg) val,
                    options(nostack, preserves_flags, readonly)
                );
                val
            };
            owner.matches_thread_id(tid)
        };
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        let is_owner = owner.matches(heap_ptr);

        if is_owner {
            let alloc = unsafe { &mut *self.allocator.get() };
            if alloc.is_allocating {
                // Re-entrancy fallback.
                unsafe {
                    page.thread_free.push::<P>(NonNull::new_unchecked(block));
                }
                return;
            }

            // Fast path local free inline
            let page_free = page.free;
            let page_alloc_count = page.alloc_count;
            let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
                unsafe { (*segment).keys[page_index] }
            } else {
                0
            };
            let is_not_full = page.list_state != 2;
            if is_not_full && (page_alloc_count != 1 || unsafe { (*segment).is_current }) {
                unsafe {
                    (*block).set_next::<P>(page_free, cookie);
                    page.free = Some(NonNull::new_unchecked(block));
                    page.alloc_count = page_alloc_count - 1;
                }
                return;
            }

            alloc.is_allocating = true;
            unsafe {
                do_local_free_internal::<P, B>(alloc, block, page, segment);
            }
            alloc.is_allocating = false;
        } else {
            // It belongs to a different heap or thread allocator. Push to thread_free atomically.
            unsafe {
                page.thread_free.push::<P>(NonNull::new_unchecked(block));
            }
        }
    }

    /// Reallocates a memory block from this heap.
    ///
    /// # Safety
    ///
    /// The pointer must be non-null and previously allocated by this heap.
    #[inline(always)]
    pub unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ensure_options_initialized();
        if new_size == 0 {
            if !ptr.is_null() {
                unsafe {
                    self.free(ptr);
                }
            }
            return core::ptr::null_mut();
        }
        if ptr.is_null() {
            return self.alloc(Layout::from_size_align(new_size, layout.align()).unwrap_or(layout));
        }

        if !P::ZERO_INITIALIZE && !P::ENABLE_POISONING {
            if new_size <= layout.size() {
                return ptr;
            }
            if layout.size() <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                if mnemosyne_local::internal::small_realloc_fits_existing_class(layout, new_size) {
                    return ptr;
                }
            } else {
                let current_usable = unsafe { mnemosyne_local::usable_size(ptr) };
                if new_size <= current_usable {
                    return ptr;
                }
            }
        }

        let new_ptr =
            self.alloc(Layout::from_size_align(new_size, layout.align()).unwrap_or(layout));
        if new_ptr.is_null() {
            return core::ptr::null_mut();
        }

        unsafe {
            core::ptr::copy_nonoverlapping(ptr, new_ptr, core::cmp::min(layout.size(), new_size));
            self.free(ptr);
        }
        new_ptr
    }
}
