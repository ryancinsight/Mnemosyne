use crate::brand::{BrandedBlock, BrandedCell, ThreadLocalToken};
use crate::Heap;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use core::ptr::NonNull;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::internal::HasSegmentPool;
use mnemosyne_local::LocalAllocatorSelector;

/// A uniquely owned, safe pointer to heap-allocated memory of type `T` from a `Heap`.
///
/// Automatically drops `T` and deallocates the memory back to the heap on drop.
pub struct BrandedBox<
    'brand,
    'heap,
    T: ?Sized,
    P: AllocPolicy = mnemosyne_core::StandardPolicy,
    B: HasSegmentPool + LocalAllocatorSelector<B> = mnemosyne_backend::MemoryBackendWrapper,
> {
    pub(crate) ptr: NonNull<T>,
    pub(crate) heap: &'heap Heap<'brand, P, B>,
    pub(crate) _non_send_sync: core::marker::PhantomData<*mut ()>,
}

impl<'brand, 'heap, T, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    BrandedBox<'brand, 'heap, T, P, B>
{
    /// Creates a new `BrandedBox` containing `val` allocated from the given `Heap`.
    #[inline(always)]
    pub fn new(
        heap: &'heap Heap<'brand, P, B>,
        token: &ThreadLocalToken<'brand>,
        val: T,
    ) -> Option<Self> {
        if core::mem::size_of::<T>() == 0 {
            let ptr: NonNull<T> = NonNull::dangling();
            // SAFETY: `T` is zero-sized, so `NonNull::dangling()` is a valid,
            // aligned pointer for a zero-byte write. `write` moves `val` into the
            // (zero-sized) location, conceptually transferring ownership to the
            // box; no storage is allocated, read, or aliased.
            unsafe {
                ptr.as_ptr().write(val);
            }
            return Some(Self {
                ptr,
                heap,
                _non_send_sync: core::marker::PhantomData,
            });
        }

        let block = heap.alloc_init(token, val)?;
        Some(Self {
            ptr: block.ptr,
            heap,
            _non_send_sync: core::marker::PhantomData,
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
            _marker: PhantomData,
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
        // SAFETY: `from_block` requires the block to be initialized with a valid
        // `T`. `self` is a live `BrandedBox`, whose invariant is that `self.ptr`
        // points to an initialized `T`; `into_raw` transfers that block out
        // without dropping or freeing, so the initialized-value invariant carries
        // over unchanged.
        unsafe { BrandedCell::from_block(block) }
    }

    /// Reconstructs a `BrandedBox` from a shared `BrandedCell`.
    ///
    /// # Safety
    /// The caller must ensure that no other copies of this `BrandedCell` (or pointers derived from it)
    /// are active or will be used.
    #[inline(always)]
    pub unsafe fn from_cell(heap: &'heap Heap<'brand, P, B>, cell: BrandedCell<'brand, T>) -> Self {
        Self::from_raw(heap, cell.into_block())
    }

    /// Reconstructs a `BrandedBox` from a raw block.
    ///
    /// # Safety
    /// The memory block must be initialized with a valid value of type `T`.
    #[inline(always)]
    pub unsafe fn from_raw(
        heap: &'heap Heap<'brand, P, B>,
        block: BrandedBlock<'brand, T>,
    ) -> Self {
        Self {
            ptr: block.ptr,
            heap,
            _non_send_sync: core::marker::PhantomData,
        }
    }
}

impl<'brand, 'heap, T: ?Sized, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Deref
    for BrandedBox<'brand, 'heap, T, P, B>
{
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        // SAFETY: the `BrandedBox` invariant guarantees `self.ptr` points to an
        // initialized, live, aligned `T` owned by this box. `&self` ties the
        // returned reference's lifetime to the borrow, and `BrandedBox` is
        // `!Send`/`!Sync`, so no aliasing mutable access can occur concurrently.
        unsafe { self.ptr.as_ref() }
    }
}

impl<'brand, 'heap, T: ?Sized, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    DerefMut for BrandedBox<'brand, 'heap, T, P, B>
{
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: the `BrandedBox` invariant guarantees `self.ptr` points to an
        // initialized, live, aligned `T` uniquely owned by this box. `&mut self`
        // proves exclusive access, so the returned unique reference cannot alias
        // any other reference for its lifetime.
        unsafe { self.ptr.as_mut() }
    }
}

impl<'brand, 'heap, T: ?Sized, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Drop
    for BrandedBox<'brand, 'heap, T, P, B>
{
    #[inline]
    fn drop(&mut self) {
        // SAFETY: the `BrandedBox` invariant guarantees `self.ptr` points to an
        // initialized, live `T` (possibly unsized) uniquely owned by this box.
        // `as_ref` reads the metadata to compute the value's size; `drop_in_place`
        // runs the value's destructor exactly once (drop is invoked at most once
        // per box). The block is freed only for non-ZST values (`size != 0`),
        // because ZST values were never allocated (their pointer is the dangling
        // sentinel); the live block is freed exactly once back to `self.heap`.
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
    pub fn clone_in(&self, token: &ThreadLocalToken<'brand>) -> Option<Self> {
        Self::new(self.heap, token, (**self).clone())
    }
}

impl<
        'brand,
        'heap,
        T: ?Sized + core::fmt::Debug,
        P: AllocPolicy,
        B: HasSegmentPool + LocalAllocatorSelector<B>,
    > core::fmt::Debug for BrandedBox<'brand, 'heap, T, P, B>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&**self, f)
    }
}

impl<
        'brand,
        'heap,
        T: ?Sized + core::fmt::Display,
        P: AllocPolicy,
        B: HasSegmentPool + LocalAllocatorSelector<B>,
    > core::fmt::Display for BrandedBox<'brand, 'heap, T, P, B>
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

impl<
        'brand,
        'heap,
        T: ?Sized + PartialEq,
        P: AllocPolicy,
        B: HasSegmentPool + LocalAllocatorSelector<B>,
    > PartialEq for BrandedBox<'brand, 'heap, T, P, B>
{
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
impl<
        'brand,
        'heap,
        T: ?Sized + Eq,
        P: AllocPolicy,
        B: HasSegmentPool + LocalAllocatorSelector<B>,
    > Eq for BrandedBox<'brand, 'heap, T, P, B>
{
}

impl<
        'brand,
        'heap,
        T: ?Sized + PartialOrd,
        P: AllocPolicy,
        B: HasSegmentPool + LocalAllocatorSelector<B>,
    > PartialOrd for BrandedBox<'brand, 'heap, T, P, B>
{
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        (**self).partial_cmp(&**other)
    }
}
impl<
        'brand,
        'heap,
        T: ?Sized + Ord,
        P: AllocPolicy,
        B: HasSegmentPool + LocalAllocatorSelector<B>,
    > Ord for BrandedBox<'brand, 'heap, T, P, B>
{
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        (**self).cmp(&**other)
    }
}
impl<
        'brand,
        'heap,
        T: ?Sized + core::hash::Hash,
        P: AllocPolicy,
        B: HasSegmentPool + LocalAllocatorSelector<B>,
    > core::hash::Hash for BrandedBox<'brand, 'heap, T, P, B>
{
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}
