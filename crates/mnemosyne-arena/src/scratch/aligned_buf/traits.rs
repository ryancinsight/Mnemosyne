use super::AlignedBuf;
use crate::scratch::element::ScratchElement;

impl<T: ScratchElement, const N: usize> Default for AlignedBuf<T, N> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ScratchElement + core::fmt::Debug, const N: usize> core::fmt::Debug for AlignedBuf<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.as_slice().iter()).finish()
    }
}

impl<T: ScratchElement + PartialEq, const N: usize> PartialEq for AlignedBuf<T, N> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: ScratchElement + Eq, const N: usize> Eq for AlignedBuf<T, N> {}

impl<T: ScratchElement + PartialOrd, const N: usize> PartialOrd for AlignedBuf<T, N> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}

impl<T: ScratchElement + Ord, const N: usize> Ord for AlignedBuf<T, N> {
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

impl<T: ScratchElement + core::hash::Hash, const N: usize> core::hash::Hash for AlignedBuf<T, N> {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl<T: ScratchElement, const N: usize> core::ops::Deref for AlignedBuf<T, N> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: ScratchElement, const N: usize> core::ops::DerefMut for AlignedBuf<T, N> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: ScratchElement, const N: usize> AsRef<[T]> for AlignedBuf<T, N> {
    #[inline]
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: ScratchElement, const N: usize> AsMut<[T]> for AlignedBuf<T, N> {
    #[inline]
    fn as_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: ScratchElement, const N: usize> core::borrow::Borrow<[T]> for AlignedBuf<T, N> {
    #[inline]
    fn borrow(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: ScratchElement, const N: usize> core::borrow::BorrowMut<[T]> for AlignedBuf<T, N> {
    #[inline]
    fn borrow_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl<T: ScratchElement + PartialEq, const N: usize> PartialEq<[T]> for AlignedBuf<T, N> {
    #[inline]
    fn eq(&self, other: &[T]) -> bool {
        self.as_slice() == other
    }
}

impl<'a, T: ScratchElement, const N: usize> IntoIterator for &'a AlignedBuf<T, N> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'a, T: ScratchElement, const N: usize> IntoIterator for &'a mut AlignedBuf<T, N> {
    type Item = &'a mut T;
    type IntoIter = core::slice::IterMut<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.as_mut_slice().iter_mut()
    }
}

impl<T: ScratchElement, const N: usize> core::iter::FromIterator<T> for AlignedBuf<T, N> {
    /// Collects up to `N` elements; any beyond `N` are silently dropped.
    ///
    /// This mirrors [`from_slice_truncating`][AlignedBuf::from_slice_truncating]:
    /// the buffer's fixed capacity cannot grow.
    #[inline]
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut buf = Self::new();
        for item in iter {
            if !buf.try_push(item) {
                break;
            }
        }
        buf
    }
}

impl<T: ScratchElement, const N: usize> From<[T; N]> for AlignedBuf<T, N> {
    #[inline]
    fn from(arr: [T; N]) -> Self {
        Self::from_array(arr)
    }
}

impl<T: ScratchElement, const N: usize> From<AlignedBuf<T, N>> for [T; N] {
    /// Converts a full buffer into an array.
    ///
    /// # Panics
    ///
    /// Panics if `buf.len() != N`.
    #[inline]
    fn from(buf: AlignedBuf<T, N>) -> [T; N] {
        assert!(
            buf.len == N,
            "AlignedBuf: cannot convert to [T; N], len {} != {N}",
            buf.len
        );
        // SAFETY: `buf.len == N` means all N slots are initialized.
        // `T: ScratchElement: Copy` so reading each by copy is valid.
        core::array::from_fn(|i| unsafe { buf.data[i].assume_init_read() })
    }
}
