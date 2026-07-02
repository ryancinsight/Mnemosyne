use crate::BrandedVec;
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
        if self.cap > 0 || (core::mem::size_of::<T>() == 0 && self.len > 0) {
            // SAFETY: the guard restricts this to vectors that own resources —
            // either a live non-ZST block (`self.cap > 0`) or ZST elements whose
            // `Drop` must still run (`len > 0`). `as_mut_slice` yields the
            // initialized prefix `[0, self.len)` as a unique `&mut [T]` (this is
            // `&mut self`), so `drop_in_place` drops each element exactly once.
            // For non-ZST `T`, `self.ptr` is the live block from `self.heap`, so
            // freeing it once is sound; the ZST branch skips the free because its
            // pointer is the dangling sentinel (never allocated). `drop` runs at
            // most once per value, so nothing is freed twice.
            unsafe {
                core::ptr::drop_in_place(self.as_mut_slice());
                if core::mem::size_of::<T>() != 0 {
                    self.heap.free_raw(self.ptr.as_ptr() as *mut u8);
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
