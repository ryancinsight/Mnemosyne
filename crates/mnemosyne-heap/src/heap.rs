use crate::brand::{BrandedBlock, InvariantLifetime, ThreadLocalToken};
use crate::raw_heap::RawHeap;
use core::alloc::Layout;
use core::marker::PhantomData;
use core::ptr::NonNull;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::LocalAllocatorSelector;
use mnemosyne_local::internal::HasSegmentPool;

/// The reason a branded reallocation could not produce a replacement block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReallocFailure {
    /// `new_size` and the existing alignment cannot form a valid `Layout`.
    InvalidLayout { new_size: usize, alignment: usize },
    /// The requested replacement allocation could not be obtained.
    AllocationFailed,
}

/// A failed branded reallocation that retains ownership of the source block.
///
/// The source block is returned inside the error so allocation failure cannot
/// leak it. Call [`Self::into_block`] to recover it, or inspect [`Self::reason`]
/// before deciding how to handle the failure; the recovered block remains
/// explicitly owned and must be released through the heap when no longer
/// needed.
#[must_use]
pub struct ReallocError<'brand, T: ?Sized> {
    block: BrandedBlock<'brand, T>,
    reason: ReallocFailure,
}

impl<'brand, T: ?Sized> ReallocError<'brand, T> {
    #[inline]
    fn new(block: BrandedBlock<'brand, T>, reason: ReallocFailure) -> Self {
        Self { block, reason }
    }

    /// Returns the failure classification without consuming the source block.
    #[inline]
    #[must_use]
    pub fn reason(&self) -> ReallocFailure {
        self.reason
    }

    /// Recovers the original source block without dropping or deallocating it.
    #[inline]
    #[must_use]
    pub fn into_block(self) -> BrandedBlock<'brand, T> {
        self.block
    }
}

impl<'brand, T: ?Sized> core::fmt::Debug for ReallocError<'brand, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReallocError")
            .field("block", &self.block)
            .field("reason", &self.reason)
            .finish()
    }
}

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

// SAFETY: `Heap<'brand, P, B>` wraps a `RawHeap<P, B>` whose only interior
// state is a `core::cell::UnsafeCell<ThreadAllocator<B>>` accessed exclusively
// through `&self` methods that assume single-threaded ownership and perform no
// internal synchronization. The brand model makes the heap thread-confined:
// every operation requires a `ThreadLocalToken<'brand>`, which melinoe mints as
// `!Send + !Sync`, so the heap cannot be *used* on another thread even if the
// `Heap` value itself is moved across one. The `RawHeap` core already carries
// the matching `unsafe impl<P, B: HasSegmentPool> Send for RawHeap<P, B>` (see
// `raw_heap.rs`) for the same reason — it is `!Send` only because
// `UnsafeCell<T>: !Sync` denies the auto-derive, not because concurrent access
// is sound. This mirrors the `unsafe impl Send for TieredHeap` reasoning in
// `tiered_heap.rs`.
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
    ///
    /// # Safety
    /// `ptr` must be a non-ZST block previously allocated by this heap's
    /// `raw` core and not yet freed; passing a foreign, dangling, or
    /// double-freed pointer is undefined behavior.
    #[inline(always)]
    pub(crate) unsafe fn free_raw(&self, ptr: *mut u8) {
        // SAFETY: by this function's own contract `ptr` is a non-ZST block
        // previously allocated by `self.raw` and not yet freed, satisfying
        // `free_owned_unchecked`'s requirement of an owned, live allocation.
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
        // SAFETY: `block` is a `BrandedBlock<'brand, T>`, and the matching
        // `&mut ThreadLocalToken<'brand>` proves exclusive access for this
        // brand, so `ptr` points to a live, fully-initialized `T` uniquely
        // owned here. `drop_in_place` runs `T::drop` exactly once; the block is
        // consumed by value so the pointer is never reused. `size_of_val`
        // reads only the layout of the live `T`, and a non-ZST block was
        // allocated by this same heap, satisfying `free_raw`'s contract.
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
        // SAFETY: `block` is a `BrandedBlock<'brand, T>` consumed by value with
        // the matching exclusive `&mut ThreadLocalToken<'brand>`, so `ptr` is a
        // live block uniquely owned here. The value is intentionally not
        // dropped (uninitialized / manually-managed memory). `size_of_val`
        // reads only `T`'s layout, and a non-ZST block was allocated by this
        // heap, satisfying `free_raw`'s contract.
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
            // SAFETY: `T` is a ZST (`size_of::<T>() == 0`), so a properly
            // aligned dangling pointer is a valid place for a write of zero
            // bytes; `write` moves `val` without reading the destination and
            // does not dereference real storage.
            unsafe {
                ptr.as_ptr().write(val);
            }
            return Some(BrandedBlock {
                ptr,
                _marker: PhantomData,
            });
        }

        let block = self.alloc(token, Layout::new::<T>())?;
        // SAFETY: `block` is freshly allocated for `Layout::new::<T>()`, so it
        // is sized and aligned for `T` (cast layout contract), and it is
        // uninitialized `T` storage that is written with a valid `T`
        // immediately below — before any path can read or drop it as a `T`
        // (cast initialization/drop contract).
        let casted = unsafe { block.cast::<T>() };
        // SAFETY: `block` was just allocated by `self.alloc` for
        // `Layout::new::<T>()`, so `casted.as_ptr()` is non-null, sized and
        // aligned for `T` and points to uninitialized owned storage;
        // `write` initializes it by moving `val` in without dropping the
        // (uninitialized) previous contents.
        unsafe {
            casted.as_ptr().write(val);
        }
        Some(casted)
    }

    /// Reallocates a memory block from this heap.
    ///
    /// The source block is returned inside [`ReallocError`] when the requested
    /// layout is invalid or replacement allocation fails, so failure does not
    /// leak or invalidate the original allocation. A `new_size` of zero frees
    /// the source block and returns `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Returns [`ReallocError`] with the original source block when the new
    /// layout is invalid or replacement allocation fails.
    #[inline(always)]
    pub fn realloc<T: ?Sized>(
        &self,
        token: &mut ThreadLocalToken<'brand>,
        block: BrandedBlock<'brand, T>,
        layout: Layout,
        new_size: usize,
    ) -> Result<Option<BrandedBlock<'brand, u8>>, ReallocError<'brand, T>> {
        let ptr = block.ptr.as_ptr() as *mut u8;
        if new_size == 0 {
            self.free(token, block);
            return Ok(None);
        }

        let new_layout = match Layout::from_size_align(new_size, layout.align()) {
            Ok(new_layout) => new_layout,
            Err(_) => {
                return Err(ReallocError::new(
                    block,
                    ReallocFailure::InvalidLayout {
                        new_size,
                        alignment: layout.align(),
                    },
                ));
            }
        };

        // ZSTs have no allocator-owned source mapping. The source value stays
        // recoverable if the replacement allocation fails, just like a normal
        // block below.
        // SAFETY: `block` is a live branded block and the matching token
        // proves exclusive access, so reading its value layout is valid.
        let is_zst = unsafe { core::mem::size_of_val(&*block.ptr.as_ptr()) == 0 };
        if layout.size() == 0 || is_zst {
            return self
                .alloc(token, new_layout)
                .map(Some)
                .ok_or_else(|| ReallocError::new(block, ReallocFailure::AllocationFailed));
        }

        let marker = block._marker;
        // SAFETY: the ZST/zero-size cases returned above, so `ptr` is a non-ZST
        // block previously allocated by `self.raw` and not yet freed, and
        // `layout` is its current layout; `&mut token` proves exclusive brand
        // access. This satisfies `realloc_owned_unchecked`'s contract, which
        // either grows/shrinks in place or moves the bytes and frees the old
        // allocation, returning the (possibly relocated) block.
        let new_ptr = unsafe { self.raw.realloc_owned_unchecked(ptr, layout, new_layout) };
        match NonNull::new(new_ptr) {
            Some(ptr) => Ok(Some(BrandedBlock {
                ptr,
                _marker: marker,
            })),
            None => Err(ReallocError::new(block, ReallocFailure::AllocationFailed)),
        }
    }
}
