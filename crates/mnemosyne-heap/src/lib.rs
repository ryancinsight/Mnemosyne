#![no_std]

extern crate alloc as std_alloc;

use core::alloc::Layout;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::LocalAllocatorSelector;
use mnemosyne_local::internal::{
    ensure_options_initialized, is_valid_layout_alloc_request, allocate_large_or_huge,
    initialize_allocated_bytes, poison_freed_bytes, deallocate_large_or_huge,
    do_local_free_internal, ThreadAllocator,
    MIN_BLOCK_SIZE, MAX_SMALL_ALLOC_SIZE, SEGMENT_SIZE, PAGES_PER_SEGMENT, PAGE_SHIFT,
    size_to_class_nonzero, Segment, Block, Page, HasSegmentPool, NonNull,
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
        if !ptr.is_null() {
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
            return unsafe { allocate_large_or_huge::<B>(layout.size(), layout.align(), P::ENABLE_POISONING) };
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
                let ptr = unsafe { allocate_large_or_huge::<B>(adjusted_size, align, P::ENABLE_POISONING) };
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
        let size = if page_index == 0 || page.block_size == 0 {
            let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            unsafe { (*segment).huge_mapping_suffix_from(ptr) }
        } else {
            page.block_size
        };
        mnemosyne_prof::on_free(ptr, size);

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
        let heap_ptr = self.allocator.get() as *mut ThreadAllocator<B> as *mut core::ffi::c_void;

        if owner.matches(heap_ptr) {
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

        let new_ptr = self.alloc(Layout::from_size_align(new_size, layout.align()).unwrap_or(layout));
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
pub struct BrandedBlock<'brand, T> {
    ptr: NonNull<T>,
    _marker: Invariant<'brand>,
}

impl<'brand, T> BrandedBlock<'brand, T> {
    /// Returns the raw pointer to the block's managed memory.
    #[inline(always)]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }
}

/// A scoped, lifetime-branded memory heap.
///
/// Statically validates local block ownership on deallocation via invariant
/// lifetimes, bypassing dynamic segment ownership checks entirely.
pub struct BrandedHeap<'brand, P: AllocPolicy, B: HasSegmentPool = mnemosyne_backend::DefaultBackend> {
    allocator: core::cell::UnsafeCell<ThreadAllocator<B>>,
    _phantom: Invariant<'brand>,
    _policy: core::marker::PhantomData<P>,
}

unsafe impl<'brand, P: AllocPolicy, B: HasSegmentPool> Send for BrandedHeap<'brand, P, B> {}
unsafe impl<'brand, P: AllocPolicy, B: HasSegmentPool> Sync for BrandedHeap<'brand, P, B> {}

impl<'brand, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> BrandedHeap<'brand, P, B> {
    /// Allocates a block of memory from this branded heap.
    ///
    /// The block is tied to the heap's unique `'brand` lifetime. Returns `None`
    /// if the allocation fails.
    #[inline(always)]
    pub fn alloc(&self, _token: &AllocatorToken<'brand>, layout: Layout) -> Option<BrandedBlock<'brand, u8>> {
        let ptr = unsafe { self.alloc_inner(layout) };
        if !ptr.is_null() {
            mnemosyne_prof::on_alloc(ptr, layout.size());
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
            return unsafe { allocate_large_or_huge::<B>(layout.size(), layout.align(), P::ENABLE_POISONING) };
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
                let ptr = unsafe { allocate_large_or_huge::<B>(adjusted_size, align, P::ENABLE_POISONING) };
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

    /// Frees a block of memory back to this branded heap.
    ///
    /// Because the block is branded with the heap's unique `'brand` lifetime,
    /// it is statically guaranteed to have been allocated by this heap. We bypass
    /// the dynamic segment ownership check and execute a check-free local free.
    #[inline(always)]
    pub fn free(&self, _token: &mut AllocatorToken<'brand>, block: BrandedBlock<'brand, u8>) {
        ensure_options_initialized();
        let ptr = block.ptr.as_ptr();
        
        let ptr_val = ptr as usize;
        let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

        let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };
        let size = if page_index == 0 || page.block_size == 0 {
            let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            unsafe { (*segment).huge_mapping_suffix_from(ptr) }
        } else {
            page.block_size
        };
        mnemosyne_prof::on_free(ptr, size);

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
                page.thread_free.push::<P>(NonNull::new_unchecked(block_ptr));
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

    /// Reallocates a memory block from this heap.
    #[inline(always)]
    pub fn realloc(
        &self,
        _token: &mut AllocatorToken<'brand>,
        block: BrandedBlock<'brand, u8>,
        layout: Layout,
        new_size: usize,
    ) -> Option<BrandedBlock<'brand, u8>> {
        ensure_options_initialized();
        let ptr = block.ptr.as_ptr();
        if new_size == 0 {
            self.free(_token, block);
            return None;
        }

        if !P::ZERO_INITIALIZE && !P::ENABLE_POISONING {
            if new_size <= layout.size() {
                return Some(block);
            }
            if layout.size() <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                if mnemosyne_local::internal::small_realloc_fits_existing_class(layout, new_size) {
                    return Some(block);
                }
            } else {
                let current_usable = unsafe { mnemosyne_local::usable_size(ptr) };
                if new_size <= current_usable {
                    return Some(block);
                }
            }
        }

        let new_block = self.alloc(_token, Layout::from_size_align(new_size, layout.align()).unwrap_or(layout))?;
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, new_block.ptr.as_ptr(), core::cmp::min(layout.size(), new_size));
        }
        self.free(_token, block);
        Some(new_block)
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
    use mnemosyne_core::StandardPolicy;
    use mnemosyne_backend::MemoryBackendWrapper;

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
            let block = heap.alloc(&token, layout).expect("branded allocation failed");
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
            let block = heap.alloc(&token, layout).expect("branded allocation failed");
            let ptr = block.as_ptr();
            unsafe {
                ptr.write(99);
            }
            let new_block = heap.realloc(&mut token, block, layout, 32).expect("branded realloc failed");
            let new_ptr = new_block.as_ptr();
            unsafe {
                assert_eq!(new_ptr.read(), 99);
            }
            heap.free(&mut token, new_block);
        });
    }
}
