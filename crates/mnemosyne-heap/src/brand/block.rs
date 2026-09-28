//! [`BrandedBlock`]: a heap block branded with a compile-time unique
//! lifetime, plus its `Debug`/`Pointer`/`Eq`/`Ord`/`Hash` impls.

use core::ptr::NonNull;
use melinoe::InvariantLifetime;

/// A wrapper representing a heap block branded with a compile-time unique lifetime.
pub struct BrandedBlock<'brand, T: ?Sized> {
    pub(crate) ptr: NonNull<T>,
    pub(crate) _marker: InvariantLifetime<'brand>,
}

impl<'brand, T: ?Sized> BrandedBlock<'brand, T> {
    /// Returns the raw pointer to the block's managed memory.
    #[inline(always)]
    pub fn as_ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }
}

impl<'brand, T> BrandedBlock<'brand, T> {
    /// Casts this branded block to managed memory of a different type,
    /// preserving the brand.
    ///
    /// # Safety
    ///
    /// The returned `BrandedBlock<'brand, U>` is trusted by safe APIs that
    /// interpret the pointee as a `U`: [`crate::Heap::free`] runs
    /// `core::ptr::drop_in_place::<U>` on it and derives its deallocation
    /// path from `size_of_val` of the `U`, [`crate::Heap::realloc`] reads
    /// the pointee's layout the same way, and
    /// [`super::BrandedCell::from_block`] hands out `&U`/`&mut U`. The caller must
    /// therefore guarantee:
    ///
    /// - **Layout**: the block's allocation is at least `size_of::<U>()`
    ///   bytes and aligned to `align_of::<U>()` (e.g. it was allocated for a
    ///   layout that covers `U`), and
    /// - **Initialization/drop discipline**: either the memory holds a valid
    ///   `U` before any path reads or drops it as one, or the block is
    ///   treated as uninitialized `U` storage — written with a valid `U`
    ///   before such a path (as [`crate::Heap::alloc_init`] does), or
    ///   released exclusively through the non-dropping
    ///   [`crate::Heap::free_uninit`].
    ///
    /// Violating either (for example casting an initialized `usize` block to
    /// `String` and freeing it) is a transmute-and-drop and undefined
    /// behavior.
    #[inline(always)]
    pub unsafe fn cast<U>(self) -> BrandedBlock<'brand, U> {
        BrandedBlock {
            ptr: self.ptr.cast(),
            _marker: self._marker,
        }
    }
}

impl<'brand, T: ?Sized> core::fmt::Debug for BrandedBlock<'brand, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("BrandedBlock")
            .field(&self.ptr.as_ptr())
            .finish()
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
        self.ptr
            .as_ptr()
            .cast::<()>()
            .cmp(&other.ptr.as_ptr().cast::<()>())
    }
}
impl<'brand, T: ?Sized> core::hash::Hash for BrandedBlock<'brand, T> {
    #[inline(always)]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.ptr.hash(state);
    }
}
