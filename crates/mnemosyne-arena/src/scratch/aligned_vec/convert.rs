//! Conversion and collection traits for [`AlignedVec`].

use super::AlignedVec;
use super::ScratchElement;

impl<T: ScratchElement> From<&[T]> for AlignedVec<T> {
    /// Creates an `AlignedVec<T>` by copying elements from a slice.
    ///
    /// Equivalent to [`AlignedVec::from_slice`] and provided here to satisfy
    /// the standard `From<&[T]>` convention that lets `into()` calls work
    /// uniformly at conversion boundaries.
    #[inline]
    fn from(slice: &[T]) -> Self {
        Self::from_slice(slice)
    }
}

impl<T: ScratchElement> From<alloc::vec::Vec<T>> for AlignedVec<T> {
    /// Converts a `Vec<T>` into an `AlignedVec<T>` by copying the elements.
    ///
    /// The source `Vec` is dropped after the copy. A true zero-copy
    /// conversion is not possible in general because `Vec` uses the global
    /// allocator while `AlignedVec` requires a specific alignment guarantee
    /// that the global allocator does not provide. When Mnemosyne is the
    /// global allocator the performance difference is one copy operation;
    /// use [`AlignedVec::from_slice`] directly when you already have a slice.
    #[inline]
    fn from(v: alloc::vec::Vec<T>) -> Self {
        Self::from_slice(&v)
    }
}

impl<T: ScratchElement> From<AlignedVec<T>> for alloc::vec::Vec<T> {
    /// Converts an `AlignedVec<T>` into a `Vec<T>` by copying the elements.
    ///
    /// Delegates to [`AlignedVec::into_vec`].
    #[inline]
    fn from(v: AlignedVec<T>) -> alloc::vec::Vec<T> {
        v.into_vec()
    }
}

impl<T: ScratchElement + PartialEq> PartialEq<alloc::vec::Vec<T>> for AlignedVec<T> {
    #[inline]
    fn eq(&self, other: &alloc::vec::Vec<T>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: ScratchElement + PartialEq> PartialEq<AlignedVec<T>> for alloc::vec::Vec<T> {
    #[inline]
    fn eq(&self, other: &AlignedVec<T>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: ScratchElement + PartialEq, const N: usize> PartialEq<[T; N]> for AlignedVec<T> {
    #[inline]
    fn eq(&self, other: &[T; N]) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: ScratchElement> core::convert::AsRef<[T]> for AlignedVec<T> {
    #[inline]
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: ScratchElement> core::convert::AsMut<[T]> for AlignedVec<T> {
    #[inline]
    fn as_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: ScratchElement> core::borrow::Borrow<[T]> for AlignedVec<T> {
    #[inline]
    fn borrow(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: ScratchElement> core::borrow::BorrowMut<[T]> for AlignedVec<T> {
    #[inline]
    fn borrow_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: ScratchElement> core::iter::FromIterator<T> for AlignedVec<T> {
    #[inline]
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let iter = iter.into_iter();
        let (lo, _) = iter.size_hint();
        let mut buf = AlignedVec::with_capacity(lo);
        for item in iter {
            buf.push(item);
        }
        buf
    }
}

impl<T: ScratchElement> Extend<T> for AlignedVec<T> {
    #[inline]
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.extend_from_iter(iter);
    }
}

impl<'a, T: ScratchElement> Extend<&'a T> for AlignedVec<T> {
    #[inline]
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        for &item in iter {
            self.push(item);
        }
    }
}

impl<T: ScratchElement> From<AlignedVec<T>> for alloc::boxed::Box<[T]> {
    /// Converts an `AlignedVec<T>` into a boxed slice.
    ///
    /// Copies the initialized elements into a heap allocation managed by the
    /// global allocator. This is a data copy because `AlignedVec` and `Box<[T]>`
    /// use different allocators (and possibly different alignments).
    #[inline]
    fn from(v: AlignedVec<T>) -> alloc::boxed::Box<[T]> {
        v.into_vec().into_boxed_slice()
    }
}

impl<T: ScratchElement> From<alloc::boxed::Box<[T]>> for AlignedVec<T> {
    /// Converts a boxed slice into an `AlignedVec<T>` by copying.
    #[inline]
    fn from(b: alloc::boxed::Box<[T]>) -> Self {
        Self::from_slice(&b)
    }
}
