use crate::brand::{BrandedBlock, InvariantLifetime, ThreadLocalToken};
use core::marker::PhantomData;
use crate::raw_heap::RawHeap;
use core::alloc::Layout;
use core::ptr::NonNull;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::internal::HasSegmentPool;
use mnemosyne_local::LocalAllocatorSelector;

/// A scoped, lifetime-branded memory heap.
///
/// `Heap` is the single public heap surface. It statically validates local
/// block ownership through the scoped brand lifetime while delegating all
/// allocation mechanics to the monomorphized `RawHeap` core.
pub struct Heap<'brand, P: AllocPolicy, B: HasSegmentPool = mnemosyne_backend::MemoryBackendWrapper>
{
    pub(crate) raw: RawHeap<P, B>,
    pub(crate) _phantom: InvariantLifetime<'brand>,
}

unsafe impl<'brand, P: AllocPolicy, B: HasSegmentPool> Send for Heap<'brand, P, B> {}

impl<'brand, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Heap<'brand, P, B> {
    /// Allocates a block of memory from this heap.
    ///
    /// The block is tied to the heap's unique `'brand` lifetime. Returns `None`
    /// if the allocation fails.
    #[inline(always)]
    pub fn alloc(
        &self,
        _token: &ThreadLocalToken<'brand>,
        layout: Layout,
    ) -> Option<BrandedBlock<'brand, u8>> {
        let ptr = self.raw.alloc(layout);
        NonNull::new(ptr).map(|ptr| BrandedBlock {
            ptr,
            _marker: PhantomData,
        })
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn stats(&self) -> mnemosyne_local::ThreadAllocatorStats {
        self.raw.stats()
    }

    /// Internal raw deallocation function.
    #[inline(always)]
    pub(crate) unsafe fn free_raw(&self, ptr: *mut u8) {
        unsafe { self.raw.free_owned_unchecked(ptr) };
    }

    /// Frees a block of memory back to this heap, dropping the value in-place first.
    ///
    /// Because the block is branded with the heap's unique `'brand` lifetime,
    /// it is statically guaranteed to have been allocated by this heap.
    #[inline(always)]
    pub fn free<T: ?Sized>(
        &self,
        _token: &mut ThreadLocalToken<'brand>,
        block: BrandedBlock<'brand, T>,
    ) {
        let ptr = block.ptr.as_ptr();
        unsafe {
            core::ptr::drop_in_place(ptr);
            if core::mem::size_of_val(&*ptr) != 0 {
                self.free_raw(ptr as *mut u8);
            }
        }
    }

    /// Frees a block of memory back to this heap without dropping the value.
    ///
    /// Useful for uninitialized memory or manual drop management.
    #[inline(always)]
    pub fn free_uninit<T: ?Sized>(
        &self,
        _token: &mut ThreadLocalToken<'brand>,
        block: BrandedBlock<'brand, T>,
    ) {
        let ptr = block.ptr.as_ptr();
        unsafe {
            if core::mem::size_of_val(&*ptr) != 0 {
                self.free_raw(ptr as *mut u8);
            }
        }
    }

    /// Allocates and initializes a value directly in a branded memory block.
    ///
    /// The block is guaranteed to contain a fully initialized value of type `T`.
    #[inline(always)]
    pub fn alloc_init<T>(
        &self,
        token: &ThreadLocalToken<'brand>,
        val: T,
    ) -> Option<BrandedBlock<'brand, T>> {
        if core::mem::size_of::<T>() == 0 {
            let ptr: NonNull<T> = NonNull::dangling();
            unsafe {
                ptr.as_ptr().write(val);
            }
            return Some(BrandedBlock {
                ptr,
                _marker: PhantomData,
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
        token: &mut ThreadLocalToken<'brand>,
        block: BrandedBlock<'brand, T>,
        layout: Layout,
        new_size: usize,
    ) -> Option<BrandedBlock<'brand, u8>> {
        let ptr = block.ptr.as_ptr() as *mut u8;
        if new_size == 0 {
            self.free(token, block);
            return None;
        }

        let is_zst = unsafe { core::mem::size_of_val(&*block.ptr.as_ptr()) == 0 };
        if layout.size() == 0 || is_zst {
            return self.alloc(
                token,
                Layout::from_size_align(new_size, layout.align()).unwrap_or(layout),
            );
        }

        let marker = block._marker;
        let new_ptr = unsafe { self.raw.realloc_owned_unchecked(ptr, layout, new_size) };
        NonNull::new(new_ptr).map(|ptr| BrandedBlock {
            ptr,
            _marker: marker,
        })
    }
}
