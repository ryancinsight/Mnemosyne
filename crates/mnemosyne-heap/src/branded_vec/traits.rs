use crate::BrandedVec;
use crate::heap::BlockFreeGuard;
use core::ops::{Deref, DerefMut};
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::LocalAllocatorSelector;
use mnemosyne_local::internal::HasSegmentPool;

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
        // The guard rather than a trailing free is what keeps the block from
        // leaking when an element's destructor panics: slice drop glue still
        // drops the remaining elements, but the unwind would carry straight past
        // a trailing call. `Vec` frees on both paths, and so must this.
        if self.cap > 0 || (core::mem::size_of::<T>() == 0 && self.len > 0) {
            // SAFETY: the condition restricts this to vectors that own resources
            // — a live non-ZST block (`self.cap > 0`), or ZST elements whose
            // `Drop` must still run (`len > 0`). `as_mut_slice` yields the
            // initialized prefix `[0, self.len)` as a unique `&mut [T]` (this is
            // `&mut self`), so `drop_in_place` drops each element exactly once.
            // For non-ZST `T`, `self.ptr` is the live block from `self.heap`, so
            // the guard holds exactly what its contract wants and returns it
            // once; the ZST branch has no block (its pointer is the dangling
            // sentinel, never allocated). `drop` runs at most once per value, so
            // nothing is freed twice.
            unsafe {
                if core::mem::size_of::<T>() == 0 {
                    core::ptr::drop_in_place(self.as_mut_slice());
                } else {
                    let _free = BlockFreeGuard::new(self.heap, self.ptr.as_ptr() as *mut u8);
                    core::ptr::drop_in_place(self.as_mut_slice());
                }
            }
        }
    }
}

impl<
    'brand,
    'heap,
    T: core::fmt::Debug,
    P: AllocPolicy,
    B: HasSegmentPool + LocalAllocatorSelector<B>,
> core::fmt::Debug for BrandedVec<'brand, 'heap, T, P, B>
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
impl<'brand, 'heap, T: Eq, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Eq
    for BrandedVec<'brand, 'heap, T, P, B>
{
}

impl<'brand, 'heap, T: PartialOrd, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>
    PartialOrd for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}
impl<'brand, 'heap, T: Ord, P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Ord
    for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}
impl<
    'brand,
    'heap,
    T: core::hash::Hash,
    P: AllocPolicy,
    B: HasSegmentPool + LocalAllocatorSelector<B>,
> core::hash::Hash for BrandedVec<'brand, 'heap, T, P, B>
{
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}
