//! Read-only queries, search, sort, and in-place reordering for [`AlignedVec`].
//!
//! None of the methods in this module change the buffer's length or allocate
//! new storage; they all either return information or delegate to the
//! equivalent `[T]` slice method. Separating them from the length-changing
//! operations in [`super::length`] keeps each module focused on a single
//! responsibility.

use super::AlignedVec;
use super::ScratchElement;

impl<T: ScratchElement> AlignedVec<T> {
    // ── Search ────────────────────────────────────────────────────────────────

    /// Binary search for `value` in a sorted slice.
    ///
    /// Delegates to `[T]::binary_search`; `AlignedVec::Deref` already gives
    /// access but this method improves discoverability.
    #[inline]
    pub fn binary_search(&self, value: &T) -> Result<usize, usize>
    where
        T: Ord,
    {
        self.as_slice().binary_search(value)
    }

    /// Binary search with a comparator. Delegates to `[T]::binary_search_by`.
    #[inline]
    pub fn binary_search_by<F: FnMut(&T) -> core::cmp::Ordering>(
        &self,
        f: F,
    ) -> Result<usize, usize> {
        self.as_slice().binary_search_by(f)
    }

    /// Binary search by key. Delegates to `[T]::binary_search_by_key`.
    #[inline]
    pub fn binary_search_by_key<K: Ord, F: FnMut(&T) -> K>(
        &self,
        b: &K,
        f: F,
    ) -> Result<usize, usize> {
        self.as_slice().binary_search_by_key(b, f)
    }

    /// Returns `true` if the slice contains `value`.
    ///
    /// Delegates to `[T]::contains`. For sorted data, prefer `binary_search`.
    #[inline]
    #[must_use]
    pub fn contains(&self, value: &T) -> bool
    where
        T: PartialEq,
    {
        self.as_slice().contains(value)
    }

    /// Returns the position of the first occurrence of `value`.
    #[inline]
    #[must_use]
    pub fn position(&self, value: &T) -> Option<usize>
    where
        T: PartialEq,
    {
        self.as_slice().iter().position(|x| x == value)
    }

    // ── Sort ──────────────────────────────────────────────────────────────────

    /// Sorts the initialized elements using `T`'s natural ordering.
    ///
    /// Delegates to `[T]::sort_unstable`. Provided for discoverability
    /// alongside the other in-place methods.
    #[inline]
    pub fn sort_unstable_inplace(&mut self)
    where
        T: Ord,
    {
        self.as_mut_slice().sort_unstable();
    }

    /// Sorts with a custom comparator. Delegates to `[T]::sort_unstable_by`.
    #[inline]
    pub fn sort_unstable_by<F: FnMut(&T, &T) -> core::cmp::Ordering>(&mut self, compare: F) {
        self.as_mut_slice().sort_unstable_by(compare);
    }

    /// Sorts by a key function. Delegates to `[T]::sort_unstable_by_key`.
    #[inline]
    pub fn sort_unstable_by_key<K: Ord, F: FnMut(&T) -> K>(&mut self, f: F) {
        self.as_mut_slice().sort_unstable_by_key(f);
    }

    /// Returns `true` if the slice is sorted in ascending order.
    #[inline]
    #[must_use]
    pub fn is_sorted(&self) -> bool
    where
        T: PartialOrd,
    {
        self.as_slice().windows(2).all(|w| w[0] <= w[1])
    }

    // ── Slice pattern queries ─────────────────────────────────────────────────

    /// Returns `true` if the buffer starts with `prefix`.
    #[inline]
    #[must_use]
    pub fn starts_with(&self, prefix: &[T]) -> bool
    where
        T: PartialEq,
    {
        self.as_slice().starts_with(prefix)
    }

    /// Returns `true` if the buffer ends with `suffix`.
    #[inline]
    #[must_use]
    pub fn ends_with(&self, suffix: &[T]) -> bool
    where
        T: PartialEq,
    {
        self.as_slice().ends_with(suffix)
    }

    /// Returns the buffer's content without the leading `prefix`, or `None`
    /// if it does not start with `prefix`.
    #[inline]
    #[must_use]
    pub fn strip_prefix(&self, prefix: &[T]) -> Option<&[T]>
    where
        T: PartialEq,
    {
        self.as_slice().strip_prefix(prefix)
    }

    /// Returns the buffer's content without the trailing `suffix`, or `None`
    /// if it does not end with `suffix`.
    #[inline]
    #[must_use]
    pub fn strip_suffix(&self, suffix: &[T]) -> Option<&[T]>
    where
        T: PartialEq,
    {
        self.as_slice().strip_suffix(suffix)
    }

    // ── In-place reordering ──────────────────────────────────────────────────

    /// Swaps the elements at indices `i` and `j` in-place.
    ///
    /// Delegates to `[T]::swap`. O(1).
    ///
    /// # Panics
    ///
    /// Panics if either index is out of bounds.
    #[inline]
    pub fn swap(&mut self, i: usize, j: usize) {
        self.as_mut_slice().swap(i, j);
    }

    /// Reverses the order of all initialized elements in-place. O(n).
    ///
    /// Delegates to `[T]::reverse`.
    #[inline]
    pub fn reverse_inplace(&mut self) {
        self.as_mut_slice().reverse();
    }

    /// Rotates all elements `mid` positions to the left.
    ///
    /// Element at index `mid` becomes the new first element. Equivalent to
    /// `[T]::rotate_left`. O(n).
    ///
    /// # Panics
    ///
    /// Panics if `mid > len()`.
    #[inline]
    pub fn rotate_left(&mut self, mid: usize) {
        self.as_mut_slice().rotate_left(mid);
    }

    /// Rotates all elements `k` positions to the right.
    ///
    /// Equivalent to `[T]::rotate_right`. O(n).
    ///
    /// # Panics
    ///
    /// Panics if `k > len()`.
    #[inline]
    pub fn rotate_right(&mut self, k: usize) {
        self.as_mut_slice().rotate_right(k);
    }

    // ── Element access ────────────────────────────────────────────────────────

    /// Returns a reference to the first element, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn first(&self) -> Option<&T> {
        self.as_slice().first()
    }

    /// Returns a mutable reference to the first element, or `None` if empty.
    #[inline]
    pub fn first_mut(&mut self) -> Option<&mut T> {
        self.as_mut_slice().first_mut()
    }

    /// Returns a reference to the last element, or `None` if empty.
    #[inline]
    #[must_use]
    pub fn last(&self) -> Option<&T> {
        self.as_slice().last()
    }

    /// Returns a mutable reference to the last element, or `None` if empty.
    #[inline]
    pub fn last_mut(&mut self) -> Option<&mut T> {
        self.as_mut_slice().last_mut()
    }

    // ── Windowed / chunked iteration ─────────────────────────────────────────

    /// Returns an iterator over overlapping windows of length `size`.
    ///
    /// Delegates to `[T]::windows`. Each window is a contiguous `&[T]` of
    /// exactly `size` elements.
    ///
    /// # Panics
    ///
    /// Panics if `size == 0`.
    #[inline]
    pub fn windows_iter(&self, size: usize) -> core::slice::Windows<'_, T> {
        self.as_slice().windows(size)
    }

    /// Returns an iterator over non-overlapping chunks of length `chunk_size`.
    ///
    /// Delegates to `[T]::chunks`. The last chunk may be shorter than
    /// `chunk_size` if the length is not a multiple.
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size == 0`.
    #[inline]
    pub fn chunks_iter(&self, chunk_size: usize) -> core::slice::Chunks<'_, T> {
        self.as_slice().chunks(chunk_size)
    }

    /// Returns an iterator over non-overlapping mutable chunks.
    ///
    /// Delegates to `[T]::chunks_mut`. The last chunk may be shorter.
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size == 0`.
    #[inline]
    pub fn chunks_mut(&mut self, chunk_size: usize) -> core::slice::ChunksMut<'_, T> {
        self.as_mut_slice().chunks_mut(chunk_size)
    }

    /// Returns an iterator over non-overlapping chunks of exactly `chunk_size`.
    ///
    /// Delegates to `[T]::chunks_exact`. Elements that don't fit a complete
    /// chunk are accessible via the iterator's `remainder()`.
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size == 0`.
    #[inline]
    pub fn chunks_exact(&self, chunk_size: usize) -> core::slice::ChunksExact<'_, T> {
        self.as_slice().chunks_exact(chunk_size)
    }

    /// Mutable counterpart of [`chunks_exact`][Self::chunks_exact].
    ///
    /// # Panics
    ///
    /// Panics if `chunk_size == 0`.
    #[inline]
    pub fn chunks_exact_mut(&mut self, chunk_size: usize) -> core::slice::ChunksExactMut<'_, T> {
        self.as_mut_slice().chunks_exact_mut(chunk_size)
    }

    // ── Splitting ─────────────────────────────────────────────────────────────

    /// Splits the initialized elements into two slices at `mid`.
    ///
    /// Returns `(&[0, mid), &[mid, len))`. Delegates to `[T]::split_at`.
    ///
    /// # Panics
    ///
    /// Panics if `mid > len()`.
    #[inline]
    #[must_use]
    pub fn split_at(&self, mid: usize) -> (&[T], &[T]) {
        self.as_slice().split_at(mid)
    }

    /// Mutable counterpart of [`split_at`][Self::split_at].
    #[inline]
    pub fn split_at_mut(&mut self, mid: usize) -> (&mut [T], &mut [T]) {
        self.as_mut_slice().split_at_mut(mid)
    }
}
