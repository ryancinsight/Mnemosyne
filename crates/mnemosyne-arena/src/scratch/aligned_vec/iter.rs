//! Iterator traits for [`AlignedVec`].

use super::AlignedVec;
use super::ScratchElement;

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
            // initialized. `T: ScratchElement: Copy` means reading (and
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
