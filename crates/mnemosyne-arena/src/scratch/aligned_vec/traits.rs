//! The standard traits [`AlignedVec`] implements.
//!
//! Derives cannot produce these: the buffer is a raw owning pointer, so
//! `Clone` reallocates and copies, `PartialEq` compares the initialized
//! prefix, and `Debug` prints that prefix rather than the pointer.

use super::AlignedVec;
use super::ScratchElement;

impl<T: ScratchElement> Drop for AlignedVec<T> {
    fn drop(&mut self) {
        if self.capacity > 0 {
            let layout = Self::layout_for(self.capacity);
            // SAFETY: `capacity > 0` means `self.ptr` came from `alloc`/`realloc`
            // with `layout_for(self.capacity)`, and `capacity` tracks the most
            // recent (re)allocation, so `layout` matches the live allocation's
            // size and alignment exactly. `ScratchElement` types are non-`Drop`
            // POD, so freeing the raw bytes leaks nothing.
            unsafe {
                alloc::alloc::dealloc(self.ptr as *mut u8, layout);
            }
        }
    }
}

impl<T: ScratchElement + core::fmt::Debug> core::fmt::Debug for AlignedVec<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.as_slice().iter()).finish()
    }
}

impl<T: ScratchElement> Clone for AlignedVec<T> {
    fn clone(&self) -> Self {
        let mut v = Self::with_capacity(self.len);
        v.extend_from_slice(self.as_slice());
        v
    }
}

impl<T: ScratchElement> Default for AlignedVec<T> {
    #[inline]
    fn default() -> Self {
        Self::dangling()
    }
}

impl<T: ScratchElement + PartialEq> PartialEq for AlignedVec<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: ScratchElement + PartialEq> PartialEq<[T]> for AlignedVec<T> {
    fn eq(&self, other: &[T]) -> bool {
        self.as_slice() == other
    }
}

impl<T: ScratchElement + Eq> Eq for AlignedVec<T> {}

impl<T: ScratchElement + PartialOrd> PartialOrd for AlignedVec<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}

impl<T: ScratchElement + Ord> Ord for AlignedVec<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

impl<T: ScratchElement + core::hash::Hash> core::hash::Hash for AlignedVec<T> {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

/// Consuming iterator that moves elements out of an `AlignedVec<T>`.
///
/// Created by [`AlignedVec::into_iter`] via the [`IntoIterator`] impl.
/// Iterates the initialized elements in order; the remaining tail is freed
/// when the iterator is dropped.
pub struct IntoIter<T: ScratchElement> {
    vec: AlignedVec<T>,
    pos: usize,
}

impl<T: ScratchElement> Iterator for IntoIter<T> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        if self.pos < self.vec.len() {
            // SAFETY: `pos < len` guarantees the element at `pos` is
            // initialized.  `T: ScratchElement: Copy` means reading (and
            // logically moving) it by copy is sound; no destructor runs.
            let value = unsafe { *self.vec.ptr.add(self.pos) };
            self.pos += 1;
            Some(value)
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.vec.len() - self.pos;
        (remaining, Some(remaining))
    }
}

impl<T: ScratchElement> ExactSizeIterator for IntoIter<T> {}
impl<T: ScratchElement> core::iter::FusedIterator for IntoIter<T> {}

impl<T: ScratchElement> core::iter::DoubleEndedIterator for IntoIter<T> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        let end = self.vec.len();
        if self.pos < end {
            // Logically pop from the back by reducing len.
            // SAFETY: `end - 1 < self.vec.len()` — initialized; T: Copy.
            let new_end = end - 1;
            // SAFETY: `new_end <= capacity`; element at `new_end` is initialized.
            unsafe { self.vec.set_len_unchecked(new_end) };
            let val = unsafe { core::ptr::read(self.vec.ptr.add(new_end)) };
            Some(val)
        } else {
            None
        }
    }
}

impl<T: ScratchElement> IntoIterator for AlignedVec<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;

    #[inline]
    fn into_iter(self) -> IntoIter<T> {
        IntoIter { vec: self, pos: 0 }
    }
}

impl<'a, T: ScratchElement> IntoIterator for &'a AlignedVec<T> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;

    #[inline]
    fn into_iter(self) -> core::slice::Iter<'a, T> {
        self.as_slice().iter()
    }
}

impl<'a, T: ScratchElement> IntoIterator for &'a mut AlignedVec<T> {
    type Item = &'a mut T;
    type IntoIter = core::slice::IterMut<'a, T>;

    #[inline]
    fn into_iter(self) -> core::slice::IterMut<'a, T> {
        self.as_mut_slice().iter_mut()
    }
}

// SAFETY: `AlignedVec` uniquely owns its heap buffer with no aliasing or shared
// ownership, so moving it to another thread is sound whenever the element type
// is itself `Send`.
unsafe impl<T: ScratchElement + Send> Send for AlignedVec<T> {}

// SAFETY: every shared path through `&AlignedVec<T>` — `as_slice`, `Deref` —
// yields `&[T]` and nothing else, so a shared reference grants no way to reach
// the owning `*mut T` mutably. Sharing one across threads is therefore exactly
// as sound as sharing `&[T]`, which holds when `T: Sync`.
unsafe impl<T: ScratchElement + Sync> Sync for AlignedVec<T> {}

impl<T: ScratchElement> core::ops::Deref for AlignedVec<T> {
    type Target = [T];
    #[inline]
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: ScratchElement> core::ops::DerefMut for AlignedVec<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

// ── Conversions ─────────────────────────────────────────────────────────────

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
    /// The source `Vec` is dropped after the copy.  A true zero-copy
    /// conversion is not possible in general because `Vec` uses the global
    /// allocator while `AlignedVec` requires a specific alignment guarantee
    /// that the global allocator does not provide.  When Mnemosyne is the
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

// ── Cross-type equality ──────────────────────────────────────────────────────

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

// ── Slice views (AsRef / AsMut / Borrow) ─────────────────────────────────────

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

// ── Collection ───────────────────────────────────────────────────────────────

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
