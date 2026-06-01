use crate::brand::{AllocatorToken, BrandedBlock, Invariant};
use core::alloc::Layout;
use core::ptr::NonNull;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::internal::{
    allocate_large_or_huge, deallocate_large_or_huge, do_local_free_internal,
    ensure_options_initialized, initialize_allocated_bytes, is_valid_layout_alloc_request,
    poison_freed_bytes, size_to_class_nonzero, Block, HasSegmentPool, Segment, ThreadAllocator,
    MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, PAGES_PER_SEGMENT, PAGE_SHIFT, SEGMENT_SIZE,
};
use mnemosyne_local::LocalAllocatorSelector;

/// A scoped, lifetime-branded memory heap.
///
/// Statically validates local block ownership on deallocation via invariant
/// lifetimes, bypassing dynamic segment ownership checks entirely.
pub struct BrandedHeap<
    'brand,
    P: AllocPolicy,
    B: HasSegmentPool = mnemosyne_backend::MemoryBackendWrapper,
> {
    pub(crate) allocator: core::cell::UnsafeCell<ThreadAllocator<B>>,
    pub(crate) _phantom: Invariant<'brand>,
    pub(crate) _policy: core::marker::PhantomData<P>,
}

unsafe impl<'brand, P: AllocPolicy, B: HasSegmentPool> Send for BrandedHeap<'brand, P, B> {}

impl<'brand, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    BrandedHeap<'brand, P, B>
{
    /// Allocates a block of memory from this branded heap.
    ///
    /// The block is tied to the heap's unique `'brand` lifetime. Returns `None`
    /// if the allocation fails.
    #[inline(always)]
    pub fn alloc(
        &self,
        _token: &AllocatorToken<'brand>,
        layout: Layout,
    ) -> Option<BrandedBlock<'brand, u8>> {
        let ptr = unsafe { self.alloc_inner(layout) };
        if !ptr.is_null() {
            if mnemosyne_prof::is_active() {
                mnemosyne_prof::on_alloc(ptr, layout.size());
            }
            Some(BrandedBlock {
                ptr: unsafe { NonNull::new_unchecked(ptr) },
                _marker: Invariant::new(),
            })
        } else {
            None
        }
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

    /// Internal raw deallocation function.
    #[inline(always)]
    pub(crate) unsafe fn free_raw(&self, ptr: *mut u8) {
        let ptr_val = ptr as usize;
        let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

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

        let block_ptr = ptr as *mut Block;
        let alloc = unsafe { &mut *self.allocator.get() };

        if alloc.is_allocating {
            // Re-entrancy fallback.
            unsafe {
                page.thread_free
                    .push::<P>(NonNull::new_unchecked(block_ptr));
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
                (*block_ptr).set_next::<P>(page_free, cookie);
                page.free = Some(NonNull::new_unchecked(block_ptr));
                page.alloc_count = page_alloc_count - 1;
            }
            return;
        }

        alloc.is_allocating = true;
        unsafe {
            do_local_free_internal::<P, B>(alloc, block_ptr, page, segment);
        }
        alloc.is_allocating = false;
    }

    /// Frees a block of memory back to this branded heap, dropping the value in-place first.
    ///
    /// Because the block is branded with the heap's unique `'brand` lifetime,
    /// it is statically guaranteed to have been allocated by this heap.
    #[inline(always)]
    pub fn free<T: ?Sized>(
        &self,
        _token: &mut AllocatorToken<'brand>,
        block: BrandedBlock<'brand, T>,
    ) {
        ensure_options_initialized();
        let ptr = block.ptr.as_ptr();
        unsafe {
            core::ptr::drop_in_place(ptr);
            if core::mem::size_of_val(&*ptr) != 0 {
                self.free_raw(ptr as *mut u8);
            }
        }
    }

    /// Frees a block of memory back to this branded heap without dropping the value.
    ///
    /// Useful for uninitialized memory or manual drop management.
    #[inline(always)]
    pub fn free_uninit<T: ?Sized>(
        &self,
        _token: &mut AllocatorToken<'brand>,
        block: BrandedBlock<'brand, T>,
    ) {
        ensure_options_initialized();
        let ptr = block.ptr.as_ptr();
        unsafe {
            if core::mem::size_of_val(&*ptr) == 0 {
                return;
            }
            self.free_raw(ptr as *mut u8);
        }
    }

    /// Allocates and initializes a value directly in a branded memory block.
    ///
    /// The block is guaranteed to contain a fully initialized value of type `T`.
    #[inline(always)]
    pub fn alloc_init<T>(
        &self,
        token: &AllocatorToken<'brand>,
        val: T,
    ) -> Option<BrandedBlock<'brand, T>> {
        if core::mem::size_of::<T>() == 0 {
            let ptr: NonNull<T> = NonNull::dangling();
            unsafe {
                ptr.as_ptr().write(val);
            }
            return Some(BrandedBlock {
                ptr,
                _marker: Invariant::new(),
            });
        }

        let block = self.alloc(token, Layout::new::<T>())?;
        let casted = block.cast::<T>();
        unsafe {
            casted.as_ptr().write(val);
        }
        Some(casted)
    }

    /// Reallocates a memory block from this heap.
    #[inline(always)]
    pub fn realloc<T: ?Sized>(
        &self,
        _token: &mut AllocatorToken<'brand>,
        block: BrandedBlock<'brand, T>,
        layout: Layout,
        new_size: usize,
    ) -> Option<BrandedBlock<'brand, u8>> {
        ensure_options_initialized();
        let ptr = block.ptr.as_ptr() as *mut u8;
        if new_size == 0 {
            self.free(_token, block);
            return None;
        }

        let is_zst = unsafe { core::mem::size_of_val(&*block.ptr.as_ptr()) == 0 };
        if layout.size() == 0 || is_zst {
            return self.alloc(
                _token,
                Layout::from_size_align(new_size, layout.align()).unwrap_or(layout),
            );
        }

        if !P::ZERO_INITIALIZE && !P::ENABLE_POISONING {
            if new_size <= layout.size() {
                if layout.size() <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                    if new_size >= layout.size() / 2 {
                        return Some(BrandedBlock {
                            ptr: block.ptr.cast(),
                            _marker: block._marker,
                        });
                    }
                } else {
                    let current_usable = unsafe { mnemosyne_local::usable_size(ptr) };
                    let new_adjusted = core::cmp::max(new_size, layout.align());
                    if new_adjusted <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                        if new_size >= layout.size() / 2 {
                            return Some(BrandedBlock {
                                ptr: block.ptr.cast(),
                                _marker: block._marker,
                            });
                        }
                    } else {
                        let page_size = mnemosyne_core::constants::PAGE_SIZE;
                        let new_page_rounded = (new_adjusted + page_size - 1) & !(page_size - 1);
                        if new_page_rounded >= current_usable {
                            return Some(BrandedBlock {
                                ptr: block.ptr.cast(),
                                _marker: block._marker,
                            });
                        }
                    }
                }
            } else {
                if layout.size() <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                    if mnemosyne_local::internal::small_realloc_fits_existing_class(
                        layout, new_size,
                    ) {
                        return Some(BrandedBlock {
                            ptr: block.ptr.cast(),
                            _marker: block._marker,
                        });
                    }
                } else {
                    let current_usable = unsafe { mnemosyne_local::usable_size(ptr) };
                    if new_size <= current_usable {
                        return Some(BrandedBlock {
                            ptr: block.ptr.cast(),
                            _marker: block._marker,
                        });
                    }
                }
            }
        }

        let new_block = self.alloc(
            _token,
            Layout::from_size_align(new_size, layout.align()).unwrap_or(layout),
        )?;
        unsafe {
            core::ptr::copy_nonoverlapping(
                ptr,
                new_block.ptr.as_ptr(),
                core::cmp::min(layout.size(), new_size),
            );
            self.free_raw(ptr);
        }
        Some(new_block)
    }
}
