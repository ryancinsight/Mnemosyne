#![no_std]

extern crate alloc as std_alloc;

use core::alloc::Layout;
use core::ops::{Deref, DerefMut};
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::internal::{
    allocate_large_or_huge, deallocate_large_or_huge, do_local_free_internal,
    ensure_options_initialized, initialize_allocated_bytes, is_valid_layout_alloc_request,
    poison_freed_bytes, size_to_class_nonzero, Block, HasSegmentPool, NonNull, Page, Segment,
    ThreadAllocator, MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, PAGES_PER_SEGMENT, PAGE_SHIFT,
    SEGMENT_SIZE,
};
use mnemosyne_local::LocalAllocatorSelector;

/// An explicit custom memory heap.
///
/// Threads can instantiate a `MnemosyneHeap` to manage their own isolated allocation stream.
/// When the heap is dropped, all segments owned by it are automatically reclaimed or orphaned.
pub struct MnemosyneHeap<P: AllocPolicy, B: HasSegmentPool = mnemosyne_backend::DefaultBackend> {
    allocator: core::cell::UnsafeCell<ThreadAllocator<B>>,
    _phantom: core::marker::PhantomData<P>,
}

unsafe impl<P: AllocPolicy, B: HasSegmentPool> Send for MnemosyneHeap<P, B> {}
unsafe impl<P: AllocPolicy, B: HasSegmentPool> Sync for MnemosyneHeap<P, B> {}

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
        if mnemosyne_prof::is_active() {
            if !ptr.is_null() {
                mnemosyne_prof::on_alloc(ptr, layout.size());
            }
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

        // Try fast path active page pop first (L1 heap-local)
        if let Some(mut page_ptr) = unsafe { *alloc.active_pages.get_unchecked(class) } {
            let page = unsafe { page_ptr.as_mut() };
            if let Some(block) = page.free {
                let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
                    let self_addr = page as *const Page as usize;
                    let segment_addr = self_addr & !(SEGMENT_SIZE - 1);
                    let segment = segment_addr as *mut Segment;
                    let page_index = page.index_in_segment();
                    unsafe { (*segment).keys[page_index] }
                } else {
                    0
                };
                unsafe {
                    page.free = (*block.as_ptr()).get_next::<P>(cookie);
                }
                page.alloc_count += 1;
                let ptr = block.as_ptr() as *mut u8;
                unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                return ptr;
            } else if page.initialized_blocks < page.max_blocks() {
                let idx = page.initialized_blocks;
                page.initialized_blocks += 1;
                page.alloc_count += 1;
                let page_start = page.page_start();
                let ptr = unsafe { page_start.add(idx * page.block_size) };
                unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                return ptr;
            }
        }

        alloc.is_allocating = true;
        let ptr = unsafe { alloc.alloc::<P>(adjusted_size) };
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
    #[inline(always)]
    pub fn free(&self, ptr: *mut u8) {
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
                unsafe { (*segment).huge_mapping_suffix_from(ptr) }
            } else {
                page.block_size
            };
            mnemosyne_prof::on_free(ptr, size);
        }

        if page_index == 0 || page.block_size == 0 {
            if P::ENABLE_POISONING {
                let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
                let size = unsafe { (*segment).huge_mapping_suffix_from(ptr) };
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
    #[inline(always)]
    pub fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ensure_options_initialized();
        if new_size == 0 {
            if !ptr.is_null() {
                self.free(ptr);
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
        }
        self.free(ptr);
        new_ptr
    }
}

/// A helper type representing a compile-time invariant brand lifetime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Invariant<'brand>(core::marker::PhantomData<fn(&'brand ()) -> &'brand ()>);

impl<'brand> Invariant<'brand> {
    #[inline(always)]
    pub const fn new() -> Self {
        Self(core::marker::PhantomData)
    }
}

/// A compile-time unique allocator token representing deallocation permissions.
///
/// This token is `!Send` and `!Sync`, binding it exclusively to the thread
/// that initialized the scoped brand.
pub struct AllocatorToken<'brand> {
    _marker: Invariant<'brand>,
    _non_send: core::marker::PhantomData<core::cell::Cell<&'brand ()>>,
}

impl<'brand> AllocatorToken<'brand> {
    #[inline(always)]
    unsafe fn new() -> Self {
        Self {
            _marker: Invariant::new(),
            _non_send: core::marker::PhantomData,
        }
    }
}

/// A wrapper representing a heap block branded with a compile-time unique lifetime.
pub struct BrandedBlock<'brand, T: ?Sized> {
    ptr: NonNull<T>,
    _marker: Invariant<'brand>,
}

impl<'brand, T: ?Sized> BrandedBlock<'brand, T> {
    /// Returns the raw pointer to the block's managed memory.
    #[inline(always)]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }
}

impl<'brand, T> BrandedBlock<'brand, T> {
    /// Safely casts this branded block to managed memory of a different type.
    #[inline(always)]
    pub fn cast<U>(self) -> BrandedBlock<'brand, U> {
        BrandedBlock {
            ptr: self.ptr.cast(),
            _marker: self._marker,
        }
    }
}

/// A scoped, lifetime-branded memory heap.
///
/// Statically validates local block ownership on deallocation via invariant
/// lifetimes, bypassing dynamic segment ownership checks entirely.
pub struct BrandedHeap<
    'brand,
    P: AllocPolicy,
    B: HasSegmentPool = mnemosyne_backend::MemoryBackendWrapper,
> {
    allocator: core::cell::UnsafeCell<ThreadAllocator<B>>,
    _phantom: Invariant<'brand>,
    _policy: core::marker::PhantomData<P>,
}

unsafe impl<'brand, P: AllocPolicy, B: HasSegmentPool> Send for BrandedHeap<'brand, P, B> {}
unsafe impl<'brand, P: AllocPolicy, B: HasSegmentPool> Sync for BrandedHeap<'brand, P, B> {}

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

        // Try fast path active page pop first (L1 heap-local)
        if let Some(mut page_ptr) = unsafe { *alloc.active_pages.get_unchecked(class) } {
            let page = unsafe { page_ptr.as_mut() };
            if let Some(block) = page.free {
                let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
                    let self_addr = page as *const Page as usize;
                    let segment_addr = self_addr & !(SEGMENT_SIZE - 1);
                    let segment = segment_addr as *mut Segment;
                    let page_index = page.index_in_segment();
                    unsafe { (*segment).keys[page_index] }
                } else {
                    0
                };
                unsafe {
                    page.free = (*block.as_ptr()).get_next::<P>(cookie);
                }
                page.alloc_count += 1;
                let ptr = block.as_ptr() as *mut u8;
                unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                return ptr;
            } else if page.initialized_blocks < page.max_blocks() {
                let idx = page.initialized_blocks;
                page.initialized_blocks += 1;
                page.alloc_count += 1;
                let page_start = page.page_start();
                let ptr = unsafe { page_start.add(idx * page.block_size) };
                unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                return ptr;
            }
        }

        alloc.is_allocating = true;
        let ptr = unsafe { alloc.alloc::<P>(adjusted_size) };
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
    unsafe fn free_raw(&self, ptr: *mut u8) {
        let ptr_val = ptr as usize;
        let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

        let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };
        if mnemosyne_prof::is_active() {
            let size = if page_index == 0 || page.block_size == 0 {
                let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
                unsafe { (*segment).huge_mapping_suffix_from(ptr) }
            } else {
                page.block_size
            };
            mnemosyne_prof::on_free(ptr, size);
        }

        if page_index == 0 || page.block_size == 0 {
            if P::ENABLE_POISONING {
                let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
                let size = unsafe { (*segment).huge_mapping_suffix_from(ptr) };
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
    pub fn free<T>(&self, _token: &mut AllocatorToken<'brand>, block: BrandedBlock<'brand, T>) {
        ensure_options_initialized();
        let ptr = block.ptr.as_ptr();
        unsafe {
            core::ptr::drop_in_place(ptr);
            if core::mem::size_of::<T>() != 0 {
                self.free_raw(ptr as *mut u8);
            }
        }
    }

    /// Frees a block of memory back to this branded heap without dropping the value.
    ///
    /// Useful for uninitialized memory or manual drop management.
    #[inline(always)]
    pub fn free_uninit<T>(
        &self,
        _token: &mut AllocatorToken<'brand>,
        block: BrandedBlock<'brand, T>,
    ) {
        ensure_options_initialized();
        if core::mem::size_of::<T>() == 0 {
            return;
        }
        let ptr = block.ptr.as_ptr() as *mut u8;
        unsafe {
            self.free_raw(ptr);
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
    pub fn realloc<T>(
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

        if layout.size() == 0 || core::mem::size_of::<T>() == 0 {
            return self.alloc(
                _token,
                Layout::from_size_align(new_size, layout.align()).unwrap_or(layout),
            );
        }

        if !P::ZERO_INITIALIZE && !P::ENABLE_POISONING {
            if new_size <= layout.size() {
                return Some(BrandedBlock {
                    ptr: block.ptr.cast(),
                    _marker: block._marker,
                });
            }
            if layout.size() <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                if mnemosyne_local::internal::small_realloc_fits_existing_class(layout, new_size) {
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

/// A uniquely owned, safe pointer to heap-allocated memory of type `T` from a `BrandedHeap`.
///
/// Automatically drops `T` and deallocates the memory back to the heap on drop.
pub struct BrandedBox<
    'brand,
    'heap,
    T: ?Sized,
    P: AllocPolicy = mnemosyne_core::StandardPolicy,
    B: HasSegmentPool + LocalAllocatorSelector<B> = mnemosyne_backend::MemoryBackendWrapper,
> {
    ptr: NonNull<T>,
    heap: &'heap BrandedHeap<'brand, P, B>,
    _non_send: core::marker::PhantomData<core::cell::Cell<&'brand ()>>,
}

impl<'brand, 'heap, T, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    BrandedBox<'brand, 'heap, T, P, B>
{
    /// Creates a new `BrandedBox` containing `val` allocated from the given `BrandedHeap`.
    #[inline(always)]
    pub fn new(
        heap: &'heap BrandedHeap<'brand, P, B>,
        token: &AllocatorToken<'brand>,
        val: T,
    ) -> Option<Self> {
        if core::mem::size_of::<T>() == 0 {
            let ptr: NonNull<T> = NonNull::dangling();
            unsafe {
                ptr.as_ptr().write(val);
            }
            return Some(Self {
                ptr,
                heap,
                _non_send: core::marker::PhantomData,
            });
        }

        let block = heap.alloc_init(token, val)?;
        Some(Self {
            ptr: block.ptr,
            heap,
            _non_send: core::marker::PhantomData,
        })
    }
}

impl<'brand, 'heap, T: ?Sized, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    BrandedBox<'brand, 'heap, T, P, B>
{
    /// Consumes the `BrandedBox`, returning the wrapped raw block without dropping or deallocating.
    #[inline(always)]
    pub fn into_raw(self) -> BrandedBlock<'brand, T> {
        let block = BrandedBlock {
            ptr: self.ptr,
            _marker: Invariant::new(),
        };
        core::mem::forget(self);
        block
    }

    /// Reconstructs a `BrandedBox` from a raw block.
    ///
    /// # Safety
    /// The memory block must be initialized with a valid value of type `T`.
    #[inline(always)]
    pub unsafe fn from_raw(
        heap: &'heap BrandedHeap<'brand, P, B>,
        block: BrandedBlock<'brand, T>,
    ) -> Self {
        Self {
            ptr: block.ptr,
            heap,
            _non_send: core::marker::PhantomData,
        }
    }
}

impl<'brand, 'heap, T: ?Sized, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Deref
    for BrandedBox<'brand, 'heap, T, P, B>
{
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        unsafe { self.ptr.as_ref() }
    }
}

impl<'brand, 'heap, T: ?Sized, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> DerefMut
    for BrandedBox<'brand, 'heap, T, P, B>
{
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.ptr.as_mut() }
    }
}

impl<'brand, 'heap, T: ?Sized, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Drop
    for BrandedBox<'brand, 'heap, T, P, B>
{
    #[inline]
    fn drop(&mut self) {
        unsafe {
            let size = core::mem::size_of_val(self.ptr.as_ref());
            core::ptr::drop_in_place(self.ptr.as_ptr());
            if size != 0 {
                self.heap.free_raw(self.ptr.as_ptr() as *mut u8);
            }
        }
    }
}

/// A dynamically growing array allocated from a `BrandedHeap`.
///
/// Automatically handles growth and reallocation, dropping all elements on drop.
pub struct BrandedVec<
    'brand,
    'heap,
    T,
    P: AllocPolicy = mnemosyne_core::StandardPolicy,
    B: HasSegmentPool + LocalAllocatorSelector<B> = mnemosyne_backend::MemoryBackendWrapper,
> {
    ptr: NonNull<T>,
    cap: usize,
    len: usize,
    heap: &'heap BrandedHeap<'brand, P, B>,
    _non_send: core::marker::PhantomData<core::cell::Cell<&'brand ()>>,
}

impl<'brand, 'heap, T, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    BrandedVec<'brand, 'heap, T, P, B>
{
    /// Creates a new empty `BrandedVec` backed by the given `BrandedHeap`.
    #[inline(always)]
    pub fn new(heap: &'heap BrandedHeap<'brand, P, B>) -> Self {
        Self {
            ptr: NonNull::dangling(),
            cap: if core::mem::size_of::<T>() == 0 {
                usize::MAX
            } else {
                0
            },
            len: 0,
            heap,
            _non_send: core::marker::PhantomData,
        }
    }

    /// Creates a new `BrandedVec` with space for at least `capacity` elements.
    #[inline]
    pub fn with_capacity(
        heap: &'heap BrandedHeap<'brand, P, B>,
        token: &AllocatorToken<'brand>,
        capacity: usize,
    ) -> Option<Self> {
        if capacity == 0 || core::mem::size_of::<T>() == 0 {
            return Some(Self::new(heap));
        }
        let layout = Layout::array::<T>(capacity).ok()?;
        let block = heap.alloc(token, layout)?;
        Some(Self {
            ptr: block.ptr.cast(),
            cap: capacity,
            len: 0,
            heap,
            _non_send: core::marker::PhantomData,
        })
    }

    /// Pushes an element onto the back of the vector, growing it if necessary.
    #[inline]
    pub fn push(&mut self, token: &mut AllocatorToken<'brand>, val: T) -> Result<(), T> {
        if core::mem::size_of::<T>() == 0 {
            self.len = match self.len.checked_add(1) {
                Some(len) => len,
                None => return Err(val),
            };
            unsafe {
                self.ptr.as_ptr().write(val);
            }
            return Ok(());
        }

        if self.len == self.cap {
            let new_cap = if self.cap == 0 {
                4
            } else {
                match self.cap.checked_mul(2) {
                    Some(cap) => cap,
                    None => return Err(val),
                }
            };
            let new_layout = match Layout::array::<T>(new_cap) {
                Ok(l) => l,
                Err(_) => return Err(val),
            };
            if self.cap == 0 {
                let block = match self.heap.alloc(token, new_layout) {
                    Some(b) => b,
                    None => return Err(val),
                };
                self.ptr = block.ptr.cast();
                self.cap = new_cap;
            } else {
                let old_layout = Layout::array::<T>(self.cap).unwrap();
                let block = BrandedBlock {
                    ptr: self.ptr,
                    _marker: Invariant::new(),
                };
                let new_block = match self
                    .heap
                    .realloc(token, block, old_layout, new_layout.size())
                {
                    Some(b) => b,
                    None => return Err(val),
                };
                self.ptr = new_block.ptr.cast();
                self.cap = new_cap;
            }
        }
        unsafe {
            self.ptr.as_ptr().add(self.len).write(val);
        }
        self.len += 1;
        Ok(())
    }

    /// Pops the last element from the vector, returning it or None if empty.
    #[inline(always)]
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            self.len -= 1;
            unsafe { Some(self.ptr.as_ptr().add(self.len).read()) }
        }
    }

    /// Returns the number of elements in the vector.
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the vector contains no elements.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the capacity of the vector.
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// Extracts a slice containing the entire vector.
    #[inline(always)]
    pub fn as_slice(&self) -> &[T] {
        if self.len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
        }
    }

    /// Extracts a mutable slice containing the entire vector.
    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        if self.len == 0 {
            &mut []
        } else {
            unsafe { core::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
        }
    }

    /// Converts this vector into a boxed slice, shrinking the memory allocation to fit.
    #[inline]
    pub fn into_boxed_slice(mut self, token: &mut AllocatorToken<'brand>) -> BrandedBox<'brand, 'heap, [T], P, B> {
        if core::mem::size_of::<T>() == 0 {
            let slice_ptr = unsafe {
                let raw_slice = core::slice::from_raw_parts_mut(NonNull::<T>::dangling().as_ptr(), self.len);
                NonNull::new_unchecked(raw_slice)
            };
            let heap = self.heap;
            core::mem::forget(self);
            return BrandedBox {
                ptr: slice_ptr,
                heap,
                _non_send: core::marker::PhantomData,
            };
        }

        if self.cap > self.len {
            if self.len == 0 {
                unsafe {
                    self.heap.free_raw(self.ptr.as_ptr() as *mut u8);
                }
                self.ptr = NonNull::dangling();
                self.cap = 0;
            } else {
                let old_layout = Layout::array::<T>(self.cap).unwrap();
                let new_size = Layout::array::<T>(self.len).unwrap().size();
                let block = BrandedBlock {
                    ptr: self.ptr,
                    _marker: Invariant::new(),
                };
                if let Some(new_block) = self.heap.realloc(token, block, old_layout, new_size) {
                    self.ptr = new_block.ptr.cast();
                    self.cap = self.len;
                }
            }
        }

        let slice_ptr = unsafe {
            let raw_slice = core::ptr::slice_from_raw_parts_mut(self.ptr.as_ptr(), self.len);
            NonNull::new_unchecked(raw_slice)
        };

        let heap = self.heap;
        core::mem::forget(self);

        BrandedBox {
            ptr: slice_ptr,
            heap,
            _non_send: core::marker::PhantomData,
        }
    }
}

impl<'brand, 'heap, T, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Deref
    for BrandedVec<'brand, 'heap, T, P, B>
{
    type Target = [T];
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<'brand, 'heap, T, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> DerefMut
    for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<'brand, 'heap, T, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Drop
    for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn drop(&mut self) {
        if self.cap > 0 || (core::mem::size_of::<T>() == 0 && self.len > 0) {
            unsafe {
                core::ptr::drop_in_place(self.as_mut_slice());
                if core::mem::size_of::<T>() != 0 {
                    self.heap.free_raw(self.ptr.as_ptr() as *mut u8);
                }
            }
        }
    }
}

/// A GhostCell-style shared container allowing interior mutability.
///
/// Permits shared read access and exclusive write access mediated by the `AllocatorToken`.
pub struct BrandedCell<'brand, T> {
    ptr: NonNull<T>,
    _marker: Invariant<'brand>,
}

impl<'brand, T> Clone for BrandedCell<'brand, T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<'brand, T> Copy for BrandedCell<'brand, T> {}

impl<'brand, T> BrandedCell<'brand, T> {
    /// Creates a new `BrandedCell` from a `BrandedBlock`.
    ///
    /// # Safety
    /// The block must be initialized.
    #[inline(always)]
    pub unsafe fn from_block(block: BrandedBlock<'brand, T>) -> Self {
        Self {
            ptr: block.ptr,
            _marker: block._marker,
        }
    }

    /// Accesses the value immutably using the allocator token.
    #[inline(always)]
    pub fn borrow<'a>(&self, _token: &'a AllocatorToken<'brand>) -> &'a T {
        unsafe { self.ptr.as_ref() }
    }

    /// Accesses the value mutably using the allocator token.
    #[inline(always)]
    pub fn borrow_mut<'a>(&self, _token: &'a mut AllocatorToken<'brand>) -> &'a mut T {
        unsafe { &mut *self.ptr.as_ptr() }
    }
}

/// Executes a closure with a fresh, compile-time unique branded heap and token.
pub fn scope<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>, F, R>(f: F) -> R
where
    F: for<'brand> FnOnce(BrandedHeap<'brand, P, B>, AllocatorToken<'brand>) -> R,
{
    let heap = BrandedHeap {
        allocator: core::cell::UnsafeCell::new(ThreadAllocator::new()),
        _phantom: Invariant::new(),
        _policy: core::marker::PhantomData,
    };
    let token = unsafe { AllocatorToken::new() };
    f(heap, token)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use mnemosyne_backend::MemoryBackendWrapper;
    use mnemosyne_core::StandardPolicy;

    #[test]
    fn test_heap_allocation_and_free() {
        let heap = MnemosyneHeap::<StandardPolicy, MemoryBackendWrapper>::new();
        let layout = Layout::from_size_align(32, 8).unwrap();
        let ptr = heap.alloc(layout);
        assert!(!ptr.is_null(), "heap allocation failed");

        unsafe {
            ptr.write(42);
            assert_eq!(ptr.read(), 42);
        }
        heap.free(ptr);
    }

    #[test]
    fn test_heap_realloc() {
        let heap = MnemosyneHeap::<StandardPolicy, MemoryBackendWrapper>::new();
        let layout = Layout::from_size_align(16, 8).unwrap();
        let ptr = heap.alloc(layout);
        assert!(!ptr.is_null());

        unsafe { ptr.write(99) };
        let new_ptr = heap.realloc(ptr, layout, 32);
        assert!(!new_ptr.is_null());
        unsafe {
            assert_eq!(new_ptr.read(), 99);
        }
        heap.free(new_ptr);
    }

    #[test]
    fn test_branded_heap_allocation_and_free() {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let layout = Layout::from_size_align(32, 8).unwrap();
            let block = heap
                .alloc(&token, layout)
                .expect("branded allocation failed");
            let ptr = block.as_ptr();
            assert!(!ptr.is_null());
            unsafe {
                ptr.write(42);
                assert_eq!(ptr.read(), 42);
            }
            heap.free(&mut token, block);
        });
    }

    #[test]
    fn test_branded_heap_realloc() {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let layout = Layout::from_size_align(16, 8).unwrap();
            let block = heap
                .alloc(&token, layout)
                .expect("branded allocation failed");
            let ptr = block.as_ptr();
            unsafe {
                ptr.write(99);
            }
            let new_block = heap
                .realloc(&mut token, block, layout, 32)
                .expect("branded realloc failed");
            let new_ptr = new_block.as_ptr();
            unsafe {
                assert_eq!(new_ptr.read(), 99);
            }
            heap.free(&mut token, new_block);
        });
    }

    #[test]
    fn test_branded_heap_realloc_zst_to_nonzero_skips_source_free() {
        struct Marker;

        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let before = unsafe { (&*heap.allocator.get()).stats() };
            let block = heap.alloc_init(&token, Marker).expect("ZST alloc failed");

            let new_block = heap
                .realloc(&mut token, block, Layout::new::<Marker>(), 16)
                .expect("ZST-to-nonzero realloc failed");
            let after_alloc = unsafe { (&*heap.allocator.get()).stats() };

            assert!(
                !new_block.as_ptr().is_null(),
                "realloc returned a null block"
            );
            assert_eq!(
                after_alloc.current_thread_live_allocations,
                before.current_thread_live_allocations + 1,
                "ZST source must not allocate, but nonzero destination must be live"
            );

            heap.free_uninit(&mut token, new_block);
            let after_free = unsafe { (&*heap.allocator.get()).stats() };
            assert_eq!(
                after_free.current_thread_live_allocations, before.current_thread_live_allocations,
                "nonzero destination block must be released after free_uninit"
            );
        });
    }

    #[test]
    fn test_branded_heap_realloc_zst_to_zero_drops_without_allocating() {
        ZST_DROP_COUNT.with(|c| c.set(0));
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let before = unsafe { (&*heap.allocator.get()).stats() };
            let block = heap
                .alloc_init(&token, ZstDrop)
                .expect("ZST alloc_init failed");

            let result = heap.realloc(&mut token, block, Layout::new::<ZstDrop>(), 0);
            let after = unsafe { (&*heap.allocator.get()).stats() };

            assert!(
                result.is_none(),
                "ZST-to-zero realloc must consume the block without a replacement"
            );
            assert_eq!(
                after.current_thread_live_allocations, before.current_thread_live_allocations,
                "ZST-to-zero realloc must not allocate or free a real block"
            );
            assert_eq!(
                ZST_DROP_COUNT.with(|c| c.get()),
                1,
                "ZST-to-zero realloc must drop the owned value exactly once"
            );
        });
    }

    #[test]
    fn test_branded_heap_generic_and_cast() {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let layout = Layout::from_size_align(32, 8).unwrap();
            let block: BrandedBlock<'_, u8> = heap
                .alloc(&token, layout)
                .expect("branded allocation failed");

            // Cast to i32 block
            let casted: BrandedBlock<'_, i32> = block.cast::<i32>();
            let ptr = casted.as_ptr();
            assert!(!ptr.is_null());
            unsafe {
                ptr.write(123456);
                assert_eq!(ptr.read(), 123456);
            }

            // Free generic block
            heap.free(&mut token, casted);
        });
    }

    use core::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct DropTracker<'a>(&'a AtomicUsize);
    impl<'a> Drop for DropTracker<'a> {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn test_branded_box_and_drop_tracking() {
        let counter = AtomicUsize::new(0);
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
            let bbox = BrandedBox::new(&heap, &token, DropTracker(&counter))
                .expect("BrandedBox allocation failed");
            assert_eq!(counter.load(Ordering::SeqCst), 0);
            // Drop should occur here when bbox goes out of scope.
            drop(bbox);
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn test_branded_heap_free_drops_value() {
        let counter = AtomicUsize::new(0);
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let block = heap
                .alloc_init(&token, DropTracker(&counter))
                .expect("alloc_init failed");
            assert_eq!(counter.load(Ordering::SeqCst), 0);
            heap.free(&mut token, block);
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn test_branded_heap_alloc_init_zst_drops_without_allocating() {
        ZST_DROP_COUNT.with(|c| c.set(0));
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let before = unsafe {
                (&*heap.allocator.get())
                    .stats()
                    .current_thread_owned_segments
            };
            let block = heap
                .alloc_init(&token, ZstDrop)
                .expect("ZST alloc_init failed");
            assert_eq!(
                unsafe {
                    (&*heap.allocator.get())
                        .stats()
                        .current_thread_owned_segments
                },
                before,
                "ZST alloc_init must not allocate a segment"
            );
            heap.free(&mut token, block);
            assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 1);
        });
    }

    #[test]
    fn test_branded_vec_growth_and_drop() {
        let counter = AtomicUsize::new(0);
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let mut vec = BrandedVec::new(&heap);
            assert!(vec.is_empty());
            assert_eq!(vec.len(), 0);
            assert_eq!(vec.capacity(), 0);

            // Push elements to trigger growth
            for _ in 0..10 {
                vec.push(&mut token, DropTracker(&counter)).unwrap();
            }
            assert_eq!(vec.len(), 10);
            assert!(vec.capacity() >= 10);

            // Pop half of the elements
            for _ in 0..5 {
                let popped = vec.pop();
                assert!(popped.is_some());
                drop(popped);
            }
            assert_eq!(vec.len(), 5);
            assert_eq!(counter.load(Ordering::SeqCst), 5); // 5 popped elements dropped

            // Drop vec, remainder should drop
            drop(vec);
            assert_eq!(counter.load(Ordering::SeqCst), 10); // all 10 elements dropped
        });
    }

    std::thread_local! {
        static ZST_DROP_COUNT: core::cell::Cell<usize> = core::cell::Cell::new(0);
    }

    #[derive(Debug)]
    struct ZstDrop;

    impl Drop for ZstDrop {
        fn drop(&mut self) {
            ZST_DROP_COUNT.with(|c| c.set(c.get() + 1));
        }
    }

    #[test]
    fn test_branded_box_zst_drops_without_allocating() {
        ZST_DROP_COUNT.with(|c| c.set(0));
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
            let before = unsafe {
                (&*heap.allocator.get())
                    .stats()
                    .current_thread_owned_segments
            };
            let bbox = BrandedBox::new(&heap, &token, ZstDrop).expect("ZST box allocation failed");
            let after_new = unsafe {
                (&*heap.allocator.get())
                    .stats()
                    .current_thread_owned_segments
            };
            assert_eq!(after_new, before, "ZST box must not allocate a segment");
            drop(bbox);
            assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 1);
        });
    }

    #[test]
    fn test_branded_vec_zst_uses_sentinel_capacity_and_drops_elements() {
        ZST_DROP_COUNT.with(|c| c.set(0));
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let before = unsafe {
                (&*heap.allocator.get())
                    .stats()
                    .current_thread_owned_segments
            };
            let mut vec = BrandedVec::with_capacity(&heap, &token, 8)
                .expect("ZST vector construction failed");
            assert_eq!(vec.capacity(), usize::MAX);
            assert_eq!(
                unsafe {
                    (&*heap.allocator.get())
                        .stats()
                        .current_thread_owned_segments
                },
                before,
                "ZST vector capacity must not allocate a segment"
            );

            for _ in 0..4 {
                vec.push(&mut token, ZstDrop).expect("ZST push failed");
            }
            assert_eq!(vec.len(), 4);
            drop(vec.pop());
            assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 1);
            drop(vec);
            assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 4);
        });
    }

    #[test]
    fn test_branded_vec_new_zst_preserves_capacity_invariant() {
        ZST_DROP_COUNT.with(|c| c.set(0));
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let before = unsafe {
                (&*heap.allocator.get())
                    .stats()
                    .current_thread_owned_segments
            };
            let mut vec = BrandedVec::new(&heap);

            assert_eq!(
                vec.capacity(),
                usize::MAX,
                "ZST vector constructed with new must use sentinel capacity"
            );
            vec.push(&mut token, ZstDrop).expect("ZST push failed");
            assert_eq!(vec.len(), 1);
            assert!(
                vec.len() <= vec.capacity(),
                "successful push must preserve len <= capacity"
            );
            assert_eq!(
                unsafe {
                    (&*heap.allocator.get())
                        .stats()
                        .current_thread_owned_segments
                },
                before,
                "ZST vector constructed with new must not allocate a segment"
            );

            drop(vec);
            assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 1);
        });
    }

    #[test]
    fn test_branded_cell_sharing_and_mutation() {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let block = heap.alloc_init(&token, 42).expect("alloc_init failed");
            let cell = unsafe { BrandedCell::from_block(block) };

            // Cell is Copy/Clone, create multiple copies
            let cell_copy1 = cell;
            let cell_copy2 = cell;

            // Read original value
            assert_eq!(*cell_copy1.borrow(&token), 42);
            assert_eq!(*cell_copy2.borrow(&token), 42);

            // Mutate value via mutable borrow
            *cell_copy1.borrow_mut(&mut token) = 100;

            // Verify mutation is reflected in all copies
            assert_eq!(*cell_copy2.borrow(&token), 100);
            assert_eq!(*cell.borrow(&token), 100);

            // Free the memory. BrandedCell is shared, so we cast to a block to free
            let block = BrandedBlock {
                ptr: cell.ptr,
                _marker: Invariant::new(),
            };
            heap.free(&mut token, block);
        });
    }

    #[test]
    fn test_branded_box_unsized_slice_and_drop() {
        let counter = AtomicUsize::new(0);
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let mut vec = BrandedVec::new(&heap);
            for _ in 0..5 {
                vec.push(&mut token, DropTracker(&counter)).unwrap();
            }
            assert_eq!(counter.load(Ordering::SeqCst), 0);
            
            // Convert to boxed slice
            let boxed_slice = vec.into_boxed_slice(&mut token);
            assert_eq!(boxed_slice.len(), 5);
            assert_eq!(counter.load(Ordering::SeqCst), 0);

            // Drop boxed slice
            drop(boxed_slice);
            assert_eq!(counter.load(Ordering::SeqCst), 5); // All 5 elements dropped
        });
    }

    #[test]
    fn test_branded_box_into_and_from_raw() {
        let counter = AtomicUsize::new(0);
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
            let bbox = BrandedBox::new(&heap, &token, DropTracker(&counter)).expect("BrandedBox allocation failed");
            assert_eq!(counter.load(Ordering::SeqCst), 0);

            // Convert to raw block
            let block = bbox.into_raw();
            assert_eq!(counter.load(Ordering::SeqCst), 0); // No drop yet

            // Reconstruct BrandedBox from raw block
            let bbox_reconstructed = unsafe { BrandedBox::from_raw(&heap, block) };
            assert_eq!(counter.load(Ordering::SeqCst), 0);

            // Drop reconstructed box
            drop(bbox_reconstructed);
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        });
    }
}
