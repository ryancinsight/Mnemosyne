#![no_std]

extern crate alloc as std_alloc;

use core::alloc::Layout;
use core::ops::{Deref, DerefMut};
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::internal::{
    allocate_large_or_huge, deallocate_large_or_huge, do_local_free_internal,
    ensure_options_initialized, initialize_allocated_bytes, is_valid_layout_alloc_request,
    poison_freed_bytes, size_to_class_nonzero, Block, HasSegmentPool, NonNull, Segment,
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

/// A helper type representing a compile-time invariant brand lifetime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Invariant<'brand>(core::marker::PhantomData<fn(&'brand ()) -> &'brand ()>);

impl<'brand> Default for Invariant<'brand> {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

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
    _non_send_sync: core::marker::PhantomData<*mut ()>,
}

impl<'brand> AllocatorToken<'brand> {
    #[inline(always)]
    unsafe fn new() -> Self {
        Self {
            _marker: Invariant::new(),
            _non_send_sync: core::marker::PhantomData,
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

impl<'brand, T: ?Sized> core::fmt::Debug for BrandedBlock<'brand, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("BrandedBlock").field(&self.ptr.as_ptr()).finish()
    }
}

impl<'brand, T: ?Sized> core::fmt::Pointer for BrandedBlock<'brand, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Pointer::fmt(&self.ptr.as_ptr(), f)
    }
}

impl<'brand, T: ?Sized> PartialEq for BrandedBlock<'brand, T> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.ptr.as_ptr(), other.ptr.as_ptr())
    }
}
impl<'brand, T: ?Sized> Eq for BrandedBlock<'brand, T> {}

impl<'brand, T: ?Sized> PartialOrd for BrandedBlock<'brand, T> {
    #[inline(always)]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<'brand, T: ?Sized> Ord for BrandedBlock<'brand, T> {
    #[inline(always)]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.ptr.as_ptr().cast::<()>().cmp(&other.ptr.as_ptr().cast::<()>())
    }
}
impl<'brand, T: ?Sized> core::hash::Hash for BrandedBlock<'brand, T> {
    #[inline(always)]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.ptr.hash(state);
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
    unsafe fn free_raw(&self, ptr: *mut u8) {
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

    /// Converts this `BrandedBox` into a shared `BrandedCell`.
    ///
    /// The memory remains allocated until it is manually reclaimed.
    #[inline(always)]
    pub fn into_cell(self) -> BrandedCell<'brand, T> {
        let block = self.into_raw();
        unsafe { BrandedCell::from_block(block) }
    }

    /// Reconstructs a `BrandedBox` from a shared `BrandedCell`.
    ///
    /// # Safety
    /// The caller must ensure that no other copies of this `BrandedCell` (or pointers derived from it)
    /// are active or will be used.
    #[inline(always)]
    pub unsafe fn from_cell(
        heap: &'heap BrandedHeap<'brand, P, B>,
        cell: BrandedCell<'brand, T>,
    ) -> Self {
        Self::from_raw(heap, cell.into_block())
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

impl<'brand, 'heap, T: ?Sized, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    DerefMut for BrandedBox<'brand, 'heap, T, P, B>
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

impl<'brand, 'heap, T: Clone, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    BrandedBox<'brand, 'heap, T, P, B>
{
    /// Clones the box using the given allocator token.
    ///
    /// Returns `None` if allocation fails.
    #[inline]
    pub fn clone_in(&self, token: &AllocatorToken<'brand>) -> Option<Self> {
        Self::new(self.heap, token, (**self).clone())
    }
}

impl<'brand, 'heap, T: ?Sized + core::fmt::Debug, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    core::fmt::Debug for BrandedBox<'brand, 'heap, T, P, B>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&**self, f)
    }
}

impl<'brand, 'heap, T: ?Sized + core::fmt::Display, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    core::fmt::Display for BrandedBox<'brand, 'heap, T, P, B>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&**self, f)
    }
}

impl<'brand, 'heap, T: ?Sized, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    core::fmt::Pointer for BrandedBox<'brand, 'heap, T, P, B>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Pointer::fmt(&self.ptr.as_ptr(), f)
    }
}

impl<'brand, 'heap, T: ?Sized + PartialEq, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    PartialEq for BrandedBox<'brand, 'heap, T, P, B>
{
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
impl<'brand, 'heap, T: ?Sized + Eq, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    Eq for BrandedBox<'brand, 'heap, T, P, B> {}

impl<'brand, 'heap, T: ?Sized + PartialOrd, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    PartialOrd for BrandedBox<'brand, 'heap, T, P, B>
{
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        (**self).partial_cmp(&**other)
    }
}
impl<'brand, 'heap, T: ?Sized + Ord, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    Ord for BrandedBox<'brand, 'heap, T, P, B>
{
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (**self).cmp(&**other)
    }
}
impl<'brand, 'heap, T: ?Sized + core::hash::Hash, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    core::hash::Hash for BrandedBox<'brand, 'heap, T, P, B>
{
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state);
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
    pub fn into_boxed_slice(
        mut self,
        token: &mut AllocatorToken<'brand>,
    ) -> BrandedBox<'brand, 'heap, [T], P, B> {
        if core::mem::size_of::<T>() == 0 {
            let slice_ptr = unsafe {
                let raw_slice =
                    core::slice::from_raw_parts_mut(NonNull::<T>::dangling().as_ptr(), self.len);
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
                let block = BrandedBlock {
                    ptr: self.ptr,
                    _marker: Invariant::new(),
                };
                let new_size = core::mem::size_of::<T>() * self.len;
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

    /// Converts a `BrandedBox<'brand, 'heap, [T], P, B>` back into a `BrandedVec<'brand, 'heap, T, P, B>`.
    ///
    /// This does not allocate or copy.
    #[inline]
    pub fn from_boxed_slice(boxed_slice: BrandedBox<'brand, 'heap, [T], P, B>) -> Self {
        let len = boxed_slice.len();
        let heap = boxed_slice.heap;
        let block = boxed_slice.into_raw();
        Self {
            ptr: unsafe { NonNull::new_unchecked(block.ptr.as_ptr() as *mut T) },
            cap: if core::mem::size_of::<T>() == 0 {
                usize::MAX
            } else {
                len
            },
            len,
            heap,
            _non_send: core::marker::PhantomData,
        }
    }

    /// Clears the vector, removing all values.
    ///
    /// Note that this method has no effect on the allocated capacity of the vector.
    #[inline]
    pub fn clear(&mut self) {
        self.truncate(0);
    }

    /// Shortens the vector, keeping the first `len` elements and dropping the rest.
    ///
    /// If `len` is greater than the vector's current length, this has no effect.
    #[inline]
    pub fn truncate(&mut self, len: usize) {
        if len < self.len {
            unsafe {
                let remaining = self.len - len;
                let tail = core::slice::from_raw_parts_mut(self.ptr.as_ptr().add(len), remaining);
                self.len = len;
                core::ptr::drop_in_place(tail);
            }
        }
    }

    /// Reserves capacity for at least `additional` more elements to be inserted in the vector.
    ///
    /// # Errors
    /// Returns `Err(())` if layout calculations overflow or allocation fails.
    #[inline]
    #[allow(clippy::result_unit_err)]
    pub fn reserve(&mut self, token: &mut AllocatorToken<'brand>, additional: usize) -> Result<(), ()> {
        if core::mem::size_of::<T>() == 0 {
            return Ok(());
        }
        let needed = match self.len.checked_add(additional) {
            Some(n) => n,
            None => return Err(()),
        };
        if needed <= self.cap {
            return Ok(());
        }
        let new_cap = core::cmp::max(self.cap.checked_mul(2).unwrap_or(needed), needed);
        let new_layout = Layout::array::<T>(new_cap).map_err(|_| ())?;
        if self.cap == 0 {
            let block = self.heap.alloc(token, new_layout).ok_or(())?;
            self.ptr = block.ptr.cast();
            self.cap = new_cap;
        } else {
            let old_layout = Layout::array::<T>(self.cap).unwrap();
            let block = BrandedBlock {
                ptr: self.ptr,
                _marker: Invariant::new(),
            };
            let new_block = self
                .heap
                .realloc(token, block, old_layout, new_layout.size())
                .ok_or(())?;
            self.ptr = new_block.ptr.cast();
            self.cap = new_cap;
        }
        Ok(())
    }

    /// Shrinks the capacity of the vector as much as possible.
    ///
    /// # Errors
    /// Returns `Err(())` if allocation fails.
    #[inline]
    #[allow(clippy::result_unit_err)]
    pub fn shrink_to_fit(&mut self, token: &mut AllocatorToken<'brand>) -> Result<(), ()> {
        if core::mem::size_of::<T>() == 0 || self.cap <= self.len {
            return Ok(());
        }
        if self.len == 0 {
            unsafe {
                self.heap.free_raw(self.ptr.as_ptr() as *mut u8);
            }
            self.ptr = NonNull::dangling();
            self.cap = 0;
            return Ok(());
        }
        let old_layout = Layout::array::<T>(self.cap).map_err(|_| ())?;
        let block = BrandedBlock {
            ptr: self.ptr,
            _marker: Invariant::new(),
        };
        let new_size = core::mem::size_of::<T>() * self.len;
        if let Some(new_block) = self.heap.realloc(token, block, old_layout, new_size) {
            self.ptr = new_block.ptr.cast();
            self.cap = self.len;
            Ok(())
        } else {
            Err(())
        }
    }

    /// Inserts an element at position `index` within the vector, shifting all elements after it to the right.
    ///
    /// # Panics
    /// Panics if `index > len`.
    ///
    /// # Errors
    /// Returns `Err(element)` if growing the vector fails.
    #[inline]
    pub fn insert(
        &mut self,
        token: &mut AllocatorToken<'brand>,
        index: usize,
        element: T,
    ) -> Result<(), T> {
        assert!(index <= self.len, "insert index out of bounds");
        if self.len == self.cap && self.reserve(token, 1).is_err() {
            return Err(element);
        }
        unsafe {
            let p = self.ptr.as_ptr().add(index);
            if index < self.len {
                core::ptr::copy(p, p.add(1), self.len - index);
            }
            p.write(element);
            self.len += 1;
        }
        Ok(())
    }

    /// Removes and returns the element at position `index` within the vector, shifting all elements after it to the left.
    ///
    /// # Panics
    /// Panics if `index >= len`.
    #[inline]
    pub fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len, "remove index out of bounds");
        unsafe {
            let p = self.ptr.as_ptr().add(index);
            let val = core::ptr::read(p);
            self.len -= 1;
            if index < self.len {
                core::ptr::copy(p.add(1), p, self.len - index);
            }
            val
        }
    }

    /// Converts this vector into a shared `BrandedCell` containing a slice.
    ///
    /// The memory is shrunk to fit and remains allocated until manually reclaimed.
    #[inline(always)]
    pub fn into_cell(self, token: &mut AllocatorToken<'brand>) -> BrandedCell<'brand, [T]> {
        self.into_boxed_slice(token).into_cell()
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

impl<'brand, 'heap, T: Clone, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    BrandedVec<'brand, 'heap, T, P, B>
{
    /// Clones the vector using the given allocator token.
    ///
    /// Returns `None` if allocation fails.
    #[inline]
    pub fn clone_in(&self, token: &mut AllocatorToken<'brand>) -> Option<Self> {
        let mut new_vec = Self::with_capacity(self.heap, token, self.len())?;
        for item in self.as_slice() {
            if new_vec.push(token, item.clone()).is_err() {
                return None;
            }
        }
        Some(new_vec)
    }
}

impl<'brand, 'heap, T: core::fmt::Debug, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    core::fmt::Debug for BrandedVec<'brand, 'heap, T, P, B>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self.as_slice(), f)
    }
}

impl<'brand, 'heap, T: PartialEq, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    PartialEq for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl<'brand, 'heap, T: Eq, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    Eq for BrandedVec<'brand, 'heap, T, P, B> {}

impl<'brand, 'heap, T: PartialOrd, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    PartialOrd for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}
impl<'brand, 'heap, T: Ord, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    Ord for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}
impl<'brand, 'heap, T: core::hash::Hash, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    core::hash::Hash for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

/// A GhostCell-style shared container allowing interior mutability.
///
/// Permits shared read access and exclusive write access mediated by the `AllocatorToken`.
pub struct BrandedCell<'brand, T: ?Sized> {
    ptr: NonNull<T>,
    _marker: Invariant<'brand>,
}

impl<'brand, T: ?Sized> Clone for BrandedCell<'brand, T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<'brand, T: ?Sized> Copy for BrandedCell<'brand, T> {}

impl<'brand, T: ?Sized> BrandedCell<'brand, T> {
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

    /// Returns the raw pointer to the cell's managed memory.
    #[inline(always)]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }

    /// Consumes the `BrandedCell` (by copy) and reconstructs the `BrandedBlock`.
    ///
    /// # Safety
    /// The caller must ensure that this is the only active reference to the cell,
    /// and that no other copies of this `BrandedCell` will be used to access the memory.
    #[inline(always)]
    pub unsafe fn into_block(self) -> BrandedBlock<'brand, T> {
        BrandedBlock {
            ptr: self.ptr,
            _marker: self._marker,
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

    /// Mutably borrows two distinct cells at the same time.
    ///
    /// # Panics
    /// Panics if the two cells point to the same memory block.
    #[inline]
    pub fn borrow_mut_2<'a, U: ?Sized>(
        cell1: &'a Self,
        cell2: &'a BrandedCell<'brand, U>,
        _token: &'a mut AllocatorToken<'brand>,
    ) -> (&'a mut T, &'a mut U) {
        assert_ne!(
            cell1.ptr.as_ptr() as *const (),
            cell2.ptr.as_ptr() as *const (),
            "borrow_mut_2: cells must be distinct"
        );
        unsafe { (&mut *cell1.ptr.as_ptr(), &mut *cell2.ptr.as_ptr()) }
    }

    /// Mutably borrows three distinct cells at the same time.
    ///
    /// # Panics
    /// Panics if any of the cells point to the same memory block.
    #[inline]
    pub fn borrow_mut_3<'a, U: ?Sized, V: ?Sized>(
        cell1: &'a Self,
        cell2: &'a BrandedCell<'brand, U>,
        cell3: &'a BrandedCell<'brand, V>,
        _token: &'a mut AllocatorToken<'brand>,
    ) -> (&'a mut T, &'a mut U, &'a mut V) {
        let p1 = cell1.ptr.as_ptr() as *const ();
        let p2 = cell2.ptr.as_ptr() as *const ();
        let p3 = cell3.ptr.as_ptr() as *const ();
        assert!(
            p1 != p2 && p2 != p3 && p1 != p3,
            "borrow_mut_3: cells must be distinct"
        );
        unsafe {
            (
                &mut *cell1.ptr.as_ptr(),
                &mut *cell2.ptr.as_ptr(),
                &mut *cell3.ptr.as_ptr(),
            )
        }
    }
}

impl<'brand, T: ?Sized> core::fmt::Debug for BrandedCell<'brand, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("BrandedCell").field(&self.ptr.as_ptr()).finish()
    }
}

impl<'brand, T: ?Sized> PartialEq for BrandedCell<'brand, T> {
    #[inline(always)]
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.ptr.as_ptr(), other.ptr.as_ptr())
    }
}
impl<'brand, T: ?Sized> Eq for BrandedCell<'brand, T> {}

impl<'brand, T: ?Sized> core::hash::Hash for BrandedCell<'brand, T> {
    #[inline(always)]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.ptr.hash(state);
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
    #![allow(clippy::missing_const_for_thread_local)]
    extern crate std;
    use std::format;
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
            heap.free(ptr);
        }
    }

    #[test]
    fn test_heap_realloc() {
        let heap = MnemosyneHeap::<StandardPolicy, MemoryBackendWrapper>::new();
        let layout = Layout::from_size_align(16, 8).unwrap();
        let ptr = heap.alloc(layout);
        assert!(!ptr.is_null());

        unsafe {
            ptr.write(99);
            let new_ptr = heap.realloc(ptr, layout, 32);
            assert!(!new_ptr.is_null());
            assert_eq!(new_ptr.read(), 99);
            heap.free(new_ptr);
        }
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
            let after_zst_alloc = unsafe { (&*heap.allocator.get()).stats() };

            assert_eq!(
                after_zst_alloc.current_thread_live_allocations,
                before.current_thread_live_allocations,
                "ZST source construction must not create a live allocator block"
            );

            let new_block = heap
                .realloc(&mut token, block, Layout::new::<Marker>(), 16)
                .expect("ZST-to-nonzero realloc failed");
            let after_alloc = unsafe { (&*heap.allocator.get()).stats() };

            assert!(
                !new_block.as_ptr().is_null(),
                "realloc returned a null block"
            );
            assert!(
                after_alloc.current_thread_live_allocations
                    > after_zst_alloc.current_thread_live_allocations,
                "nonzero destination must create a live allocator block"
            );

            heap.free_uninit(&mut token, new_block);
            let after_free = unsafe { (&*heap.allocator.get()).stats() };
            assert!(
                after_free.current_thread_live_allocations
                    < after_alloc.current_thread_live_allocations,
                "free_uninit must release the nonzero destination block"
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
        static ZST_DROP_COUNT: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
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

            // Free the memory using the safe/encapsulated conversion
            heap.free(&mut token, unsafe { cell.into_block() });
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
    fn test_branded_vec_into_boxed_slice_shrinks_storage_to_len() {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let mut vec = BrandedVec::with_capacity(&heap, &token, 1024)
                .expect("oversized vector allocation failed");
            vec.push(&mut token, 0xCAFE_BABEu64)
                .expect("push into preallocated vector failed");

            let before_usable = unsafe { mnemosyne_local::usable_size(vec.as_ptr() as *mut u8) };
            let boxed_slice = vec.into_boxed_slice(&mut token);
            let after_usable =
                unsafe { mnemosyne_local::usable_size(boxed_slice.as_ptr() as *mut u8) };

            assert_eq!(boxed_slice.len(), 1);
            assert_eq!(boxed_slice[0], 0xCAFE_BABE);
            assert!(
                after_usable <= before_usable,
                "boxed slice conversion must not increase usable storage from {before_usable}, got {after_usable}"
            );
        });
    }

    #[test]
    fn test_branded_box_into_and_from_raw() {
        let counter = AtomicUsize::new(0);
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
            let bbox = BrandedBox::new(&heap, &token, DropTracker(&counter))
                .expect("BrandedBox allocation failed");
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

    #[test]
    fn test_branded_box_into_cell() {
        let counter = AtomicUsize::new(0);
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let bbox = BrandedBox::new(&heap, &token, DropTracker(&counter))
                .expect("BrandedBox allocation failed");
            assert_eq!(counter.load(Ordering::SeqCst), 0);

            // Convert to shared BrandedCell
            let cell = bbox.into_cell();
            assert_eq!(counter.load(Ordering::SeqCst), 0);

            // Read the cell
            assert_eq!(cell.borrow(&token).0.load(Ordering::SeqCst), 0);

            // Reclaim memory using the safe/encapsulated conversion
            heap.free(&mut token, unsafe { cell.into_block() });
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn test_branded_cell_unsized_slice() {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let mut vec = BrandedVec::new(&heap);
            vec.push(&mut token, 10).unwrap();
            vec.push(&mut token, 20).unwrap();
            vec.push(&mut token, 30).unwrap();

            let boxed_slice = vec.into_boxed_slice(&mut token);
            assert_eq!(boxed_slice.len(), 3);

            let cell = boxed_slice.into_cell();
            assert_eq!(cell.borrow(&token), &[10, 20, 30]);

            // Mutate cell slice elements
            cell.borrow_mut(&mut token)[1] = 99;
            assert_eq!(cell.borrow(&token), &[10, 99, 30]);

            heap.free(&mut token, unsafe { cell.into_block() });
        });
    }

    #[test]
    fn test_branded_cell_multi_mutable_borrow() {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let b1 = heap.alloc_init(&token, 1).unwrap();
            let b2 = heap.alloc_init(&token, 2.0).unwrap();
            let b3 = heap
                .alloc_init(&token, std::string::String::from("3"))
                .unwrap();

            let c1 = unsafe { BrandedCell::from_block(b1) };
            let c2 = unsafe { BrandedCell::from_block(b2) };
            let c3 = unsafe { BrandedCell::from_block(b3) };

            {
                let (r1, r2, r3) = BrandedCell::borrow_mut_3(&c1, &c2, &c3, &mut token);
                *r1 = 10;
                *r2 = 20.0;
                r3.push('0');
            }

            assert_eq!(*c1.borrow(&token), 10);
            assert_eq!(*c2.borrow(&token), 20.0);
            assert_eq!(c3.borrow(&token), "30");

            let (r1, r2) = BrandedCell::borrow_mut_2(&c1, &c2, &mut token);
            *r1 = 100;
            *r2 = 200.0;

            assert_eq!(*c1.borrow(&token), 100);
            assert_eq!(*c2.borrow(&token), 200.0);

            // Reclaim
            heap.free(&mut token, unsafe { c1.into_block() });
            heap.free(&mut token, unsafe { c2.into_block() });
            heap.free(&mut token, unsafe { c3.into_block() });
        });
    }

    #[test]
    #[should_panic(expected = "borrow_mut_2: cells must be distinct")]
    fn test_branded_cell_multi_mutable_borrow_panic() {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let b = heap.alloc_init(&token, 42).unwrap();
            let c = unsafe { BrandedCell::from_block(b) };

            // This must panic since c and c point to the same block
            let _ = BrandedCell::borrow_mut_2(&c, &c, &mut token);
        });
    }

    #[test]
    fn test_branded_cell_as_ptr_identity() {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let b1 = heap.alloc_init(&token, 42).unwrap();
            let b2 = heap.alloc_init(&token, 42).unwrap();

            let c1 = unsafe { BrandedCell::from_block(b1) };
            let c2 = unsafe { BrandedCell::from_block(b2) };
            let c1_copy = c1;

            assert_eq!(c1.as_ptr(), c1_copy.as_ptr());
            assert_ne!(c1.as_ptr(), c2.as_ptr());

            heap.free(&mut token, unsafe { c1.into_block() });
            heap.free(&mut token, unsafe { c2.into_block() });
        });
    }

    #[test]
    fn test_branded_box_from_cell() {
        let counter = std::sync::atomic::AtomicUsize::new(0);
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
            let bbox = BrandedBox::new(&heap, &token, DropTracker(&counter))
                .expect("allocation failed");
            assert_eq!(counter.load(Ordering::SeqCst), 0);

            let cell = bbox.into_cell();
            assert_eq!(counter.load(Ordering::SeqCst), 0);

            // Reconstruct box from cell
            let bbox_reconstructed = unsafe { BrandedBox::from_cell(&heap, cell) };
            assert_eq!(counter.load(Ordering::SeqCst), 0);

            drop(bbox_reconstructed);
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn test_branded_vec_from_boxed_slice_transitions() {
        let counter = std::sync::atomic::AtomicUsize::new(0);
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            // Sized types test
            let mut vec = BrandedVec::new(&heap);
            vec.push(&mut token, DropTracker(&counter)).unwrap();
            vec.push(&mut token, DropTracker(&counter)).unwrap();

            let boxed = vec.into_boxed_slice(&mut token);
            assert_eq!(boxed.len(), 2);
            assert_eq!(counter.load(Ordering::SeqCst), 0);

            let vec_recovered = BrandedVec::from_boxed_slice(boxed);
            assert_eq!(vec_recovered.len(), 2);
            assert_eq!(counter.load(Ordering::SeqCst), 0);

            drop(vec_recovered);
            assert_eq!(counter.load(Ordering::SeqCst), 2);

            // ZST test
            let mut zst_vec = BrandedVec::new(&heap);
            zst_vec.push(&mut token, ()).unwrap();
            zst_vec.push(&mut token, ()).unwrap();

            let zst_boxed = zst_vec.into_boxed_slice(&mut token);
            assert_eq!(zst_boxed.len(), 2);

            let zst_vec_recovered = BrandedVec::from_boxed_slice(zst_boxed);
            assert_eq!(zst_vec_recovered.len(), 2);
            assert_eq!(zst_vec_recovered.capacity(), usize::MAX);
        });
    }

    #[test]
    fn test_branded_vec_into_cell() {
        let counter = std::sync::atomic::AtomicUsize::new(0);
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let mut vec = BrandedVec::new(&heap);
            vec.push(&mut token, DropTracker(&counter)).unwrap();
            vec.push(&mut token, DropTracker(&counter)).unwrap();

            let cell = vec.into_cell(&mut token);
            assert_eq!(cell.borrow(&token).len(), 2);
            assert_eq!(counter.load(Ordering::SeqCst), 0);

            heap.free(&mut token, unsafe { cell.into_block() });
            assert_eq!(counter.load(Ordering::SeqCst), 2);
        });
    }

    #[test]
    fn test_branded_containers_traits_and_vec_ops() {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            // --- BrandedBlock ---
            let b1 = heap.alloc_init(&token, 42).unwrap();
            let b2 = heap.alloc_init(&token, 42).unwrap();
            
            // Pointer
            let _ = format!("{:p}", b1);
            // Debug
            let _ = format!("{:?}", b1);
            // PartialEq/Eq
            assert_eq!(b1, b1);
            assert_ne!(b1, b2);
            // PartialOrd/Ord
            assert!(b1 != b2);
            // Hash
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            use core::hash::Hash;
            b1.hash(&mut hasher);

            // --- BrandedBox ---
            let box1 = BrandedBox::new(&heap, &token, 100).unwrap();
            let box2 = BrandedBox::new(&heap, &token, 200).unwrap();
            // Display/Pointer/Debug
            let _ = format!("{}", box1);
            let _ = format!("{:p}", box1);
            let _ = format!("{:?}", box1);
            // PartialEq/Eq
            assert_eq!(box1, box1);
            assert_ne!(box1, box2);
            // PartialOrd/Ord
            assert_eq!(box1.partial_cmp(&box2), Some(core::cmp::Ordering::Less));
            assert_eq!(box1.cmp(&box2), core::cmp::Ordering::Less);
            // Hash
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            box1.hash(&mut hasher);
            // clone_in
            let box1_clone = box1.clone_in(&token).unwrap();
            assert_eq!(box1, box1_clone);

            // --- BrandedCell ---
            let cell1 = box1.into_cell();
            let cell2 = box2.into_cell();
            let cell1_clone = box1_clone.into_cell();
            // Debug
            let _ = format!("{:?}", cell1);
            // PartialEq/Eq
            assert_eq!(cell1, cell1);
            assert_ne!(cell1, cell2);
            // Hash
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            cell1.hash(&mut hasher);

            // Reclaim BrandedCells
            heap.free(&mut token, unsafe { cell1.into_block() });
            heap.free(&mut token, unsafe { cell2.into_block() });
            heap.free(&mut token, unsafe { cell1_clone.into_block() });

            // --- BrandedVec ---
            let mut vec = BrandedVec::new(&heap);
            vec.push(&mut token, 10).unwrap();
            vec.push(&mut token, 20).unwrap();
            vec.push(&mut token, 30).unwrap();

            // Debug
            let _ = format!("{:?}", vec);
            // PartialEq/Eq
            let vec_clone = vec.clone_in(&mut token).unwrap();
            assert_eq!(vec, vec_clone);
            // PartialOrd/Ord
            assert!(vec <= vec_clone);
            
            // clear
            let mut vec_clear = vec_clone;
            vec_clear.clear();
            assert_eq!(vec_clear.len(), 0);

            // truncate
            let mut vec_trunc = vec.clone_in(&mut token).unwrap();
            vec_trunc.truncate(1);
            assert_eq!(vec_trunc.len(), 1);
            assert_eq!(vec_trunc[0], 10);

            // reserve & shrink_to_fit
            let mut vec_res = vec.clone_in(&mut token).unwrap();
            vec_res.reserve(&mut token, 100).unwrap();
            assert!(vec_res.capacity() >= 103);
            vec_res.shrink_to_fit(&mut token).unwrap();
            assert_eq!(vec_res.capacity(), 3);

            // insert & remove
            let mut vec_ins = vec.clone_in(&mut token).unwrap();
            vec_ins.insert(&mut token, 1, 99).unwrap();
            assert_eq!(vec_ins.len(), 4);
            assert_eq!(vec_ins[0], 10);
            assert_eq!(vec_ins[1], 99);
            assert_eq!(vec_ins[2], 20);
            assert_eq!(vec_ins[3], 30);

            let removed = vec_ins.remove(1);
            assert_eq!(removed, 99);
            assert_eq!(vec_ins.len(), 3);
            assert_eq!(vec_ins[0], 10);
            assert_eq!(vec_ins[1], 20);
            assert_eq!(vec_ins[2], 30);

            // Clean up
            heap.free(&mut token, b1);
            heap.free(&mut token, b2);
        });
    }

    #[test]
    fn test_branded_vec_in_place_shrink() {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            // Minor shrink (within 50% threshold): should shrink in-place
            let mut vec_minor = BrandedVec::with_capacity(&heap, &token, 4).unwrap();
            vec_minor.push(&mut token, 42).unwrap();
            vec_minor.push(&mut token, 43).unwrap();
            vec_minor.push(&mut token, 44).unwrap();
            
            let orig_ptr_minor = vec_minor.as_slice().as_ptr();
            assert_eq!(vec_minor.capacity(), 4);
            
            // Shrink minor vector capacity from 4 to 3 (new_size 12 >= 16 / 2)
            vec_minor.shrink_to_fit(&mut token).unwrap();
            assert_eq!(vec_minor.len(), 3);
            assert_eq!(vec_minor.capacity(), 3);
            assert_eq!(vec_minor.as_slice().as_ptr(), orig_ptr_minor);
            
            // Major shrink (below 50% threshold): should copy & free to release memory
            let mut vec_major = BrandedVec::with_capacity(&heap, &token, 10).unwrap();
            vec_major.push(&mut token, 100).unwrap();
            vec_major.push(&mut token, 101).unwrap();
            
            let orig_ptr_major = vec_major.as_slice().as_ptr();
            assert_eq!(vec_major.capacity(), 10);
            
            // Shrink major vector capacity from 10 to 2 (new_size 8 < 40 / 2)
            vec_major.shrink_to_fit(&mut token).unwrap();
            assert_eq!(vec_major.len(), 2);
            assert_eq!(vec_major.capacity(), 2);
            assert_ne!(vec_major.as_slice().as_ptr(), orig_ptr_major);
            
            // Similar minor shrink check for into_boxed_slice
            let mut vec_slice = BrandedVec::with_capacity(&heap, &token, 4).unwrap();
            vec_slice.push(&mut token, 200).unwrap();
            vec_slice.push(&mut token, 201).unwrap();
            vec_slice.push(&mut token, 202).unwrap();
            
            let orig_ptr_slice = vec_slice.as_slice().as_ptr();
            let boxed_slice = vec_slice.into_boxed_slice(&mut token);
            assert_eq!(boxed_slice.len(), 3);
            assert_eq!((*boxed_slice).as_ptr(), orig_ptr_slice);
        });
    }
}
