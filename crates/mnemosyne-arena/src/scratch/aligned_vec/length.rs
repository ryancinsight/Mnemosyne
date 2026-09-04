//! Operations that change an [`AlignedVec`]'s length.
//!
//! Length-changing operations use the storage module's doubling growth policy,
//! so scratch reuse remains amortized while quiescent release handles retention.

use super::AlignedVec;
use super::ScratchElement;

impl<T: ScratchElement> AlignedVec<T> {
    /// Creates an `AlignedVec` of exactly `len` zero-initialized elements.
    ///
    /// Equivalent to `with_capacity(len)` followed by `ensure_len(len)`, but
    /// expressed as a single constructor. All elements are zero (valid per the
    /// [`ScratchElement`] invariant).
    #[inline]
    #[must_use]
    pub fn zeroed(len: usize) -> Self {
        let mut v = Self::with_capacity(len);
        v.ensure_len(len);
        v
    }

    /// Ensures capacity for at least `min_len` elements. Only grows; never
    /// shrinks. Only zeroes **newly** allocated elements, not existing ones.
    #[inline]
    pub fn ensure_len(&mut self, min_len: usize) {
        if min_len <= self.len {
            return;
        }
        if min_len > self.capacity {
            self.grow_geometric(min_len);
        }
        // Zero only the newly added range.
        // SAFETY: capacity was grown to `>= min_len` above, so the range
        // `[self.len, min_len)` lies fully inside the allocation. All-zero is a
        // valid bit pattern for every `ScratchElement` type (`f32`/`f64`/`u8`/
        // `eunomia::Complex`), so zeroing produces valid initialized `T`
        // values.
        unsafe {
            let dst = self.ptr.add(self.len);
            core::ptr::write_bytes(dst, 0, min_len - self.len);
        }
        self.len = min_len;
    }

    /// Creates a buffer of exactly `len` elements, every one equal to `value`.
    ///
    /// The zero-valued case has a cheaper path in [`zeroed`][Self::zeroed],
    /// which writes bytes rather than elements.
    #[inline]
    #[must_use]
    pub fn filled(len: usize, value: T) -> Self {
        let mut buffer = Self::with_capacity(len);
        buffer.resize(len, value);
        buffer
    }

    /// Creates a buffer holding a copy of `slice`.
    #[inline]
    #[must_use]
    pub fn from_slice(slice: &[T]) -> Self {
        let mut buffer = Self::with_capacity(slice.len());
        buffer.extend_from_slice(slice);
        buffer
    }

    /// Appends one element, growing the allocation if it is full.
    ///
    /// Amortized O(1): a growth doubles capacity, so `n` pushes onto an empty
    /// buffer perform O(log n) reallocations and O(n) element writes.
    #[inline]
    pub fn push(&mut self, value: T) {
        self.reserve(1);
        // SAFETY: `reserve(1)` leaves `capacity > len`, so `ptr.add(len)` is
        // inside the allocation and past the initialized prefix `[0, len)`.
        // `T: Copy` (a `ScratchElement` supertrait), so the write is the whole
        // initialization and overwrites no live value.
        unsafe { core::ptr::write(self.ptr.add(self.len), value) };
        self.len += 1;
    }

    /// Appends every element of `slice` in one bulk copy.
    #[inline]
    pub fn extend_from_slice(&mut self, slice: &[T]) {
        if slice.is_empty() {
            return;
        }
        self.reserve(slice.len());
        // SAFETY: `reserve(n)` leaves `capacity >= len + n`, so the destination
        // range `[len, len + n)` is inside the allocation and past the
        // initialized prefix. `slice` is a live initialized `[T]`, and it
        // cannot overlap that range: it is either a distinct allocation or an
        // initialized region of this one, which the destination excludes.
        unsafe {
            core::ptr::copy_nonoverlapping(slice.as_ptr(), self.ptr.add(self.len), slice.len());
        }
        self.len += slice.len();
    }

    /// Sets the length to `new_len`, filling any new elements with `value`.
    ///
    /// Shrinking keeps the allocation; `ScratchElement` is `Copy`, so the
    /// dropped tail needs no destructor run.
    #[inline]
    pub fn resize(&mut self, new_len: usize, value: T) {
        if new_len > self.len {
            let additional = new_len - self.len;
            self.reserve(additional);
            // SAFETY: `reserve(additional)` leaves `capacity >= new_len`. Each
            // write targets a distinct index in `[len, new_len)`, inside the
            // allocation and past the initialized prefix; `T: Copy`, so each
            // write fully initializes its element.
            unsafe {
                let base = self.ptr.add(self.len);
                for offset in 0..additional {
                    core::ptr::write(base.add(offset), value);
                }
            }
        }
        self.len = new_len;
    }

    /// Sets the length to `new_len`, filling any new elements using `f`.
    ///
    /// Like [`resize`][Self::resize] but uses a closure for the fill value.
    /// Shrinking keeps the allocation.
    #[inline]
    pub fn resize_with<F: FnMut() -> T>(&mut self, new_len: usize, mut f: F) {
        if new_len > self.len {
            let additional = new_len - self.len;
            self.reserve(additional);
            // SAFETY: `reserve(additional)` leaves `capacity >= new_len`. Each
            // write targets a distinct index in `[len, new_len)` inside the
            // allocation; `T: Copy`, so each write fully initializes its element.
            unsafe {
                let base = self.ptr.add(self.len);
                for offset in 0..additional {
                    core::ptr::write(base.add(offset), f());
                }
            }
        }
        self.len = new_len;
    }

    /// Overwrites every initialized element with the value produced by `f`.
    ///
    /// Unlike [`fill`][Self::fill] (which takes a `Copy` value), this version
    /// accepts a closure so non-`Copy` logic can produce each element.
    #[inline]
    pub fn fill_with<F: FnMut() -> T>(&mut self, mut f: F) {
        // SAFETY: `[0, self.len)` is fully initialized; `T: Copy` makes
        // overwriting sound.
        for i in 0..self.len {
            unsafe { core::ptr::write(self.ptr.add(i), f()) };
        }
    }

    /// Sets the length to zero, retaining the underlying allocation for reuse.
    ///
    /// Elements are not zeroed; subsequent [`push`][Self::push] or
    /// [`extend_from_slice`][Self::extend_from_slice] calls will overwrite them
    /// before exposing them through [`as_slice`][Self::as_slice].
    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Shortens the buffer to `new_len` elements, retaining the allocation.
    ///
    /// If `new_len >= self.len()`, this is a no-op.
    #[inline]
    pub fn truncate(&mut self, new_len: usize) {
        if new_len < self.len {
            self.len = new_len;
        }
    }

    /// Appends every element produced by an iterator.
    ///
    /// Forwards to [`push`][Self::push] per element; an
    /// [`extend_from_slice`][Self::extend_from_slice] call is preferred when a
    /// contiguous source slice is available.
    #[inline]
    pub fn extend_from_iter(&mut self, iter: impl IntoIterator<Item = T>) {
        for value in iter {
            self.push(value);
        }
    }

    // ── Removal ──────────────────────────────────────────────────────────────

    /// Removes and returns the last element, or `None` if empty. O(1).
    #[inline]
    #[must_use]
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        // SAFETY: `self.len` was decremented, so the element at the former last
        // position is still inside the allocation and was initialized; `T: Copy`
        // makes a bitwise read safe.
        Some(unsafe { core::ptr::read(self.ptr.add(self.len)) })
    }

    /// Removes and returns the element at `index` by swapping it with the last.
    ///
    /// Does **not** preserve element order. O(1).
    ///
    /// # Panics
    ///
    /// Panics if `index >= self.len()`.
    #[inline]
    #[must_use]
    pub fn swap_remove(&mut self, index: usize) -> T {
        assert!(
            index < self.len,
            "swap_remove: index {index} >= len {}",
            self.len
        );
        // SAFETY: both `index` and `self.len - 1` are `< self.len`, so both are
        // inside the initialized region; `T: Copy`.
        let last = unsafe { core::ptr::read(self.ptr.add(self.len - 1)) };
        let removed = unsafe { core::ptr::read(self.ptr.add(index)) };
        if index != self.len - 1 {
            // SAFETY: writing to `index < self.len` stays inside the allocation.
            unsafe { core::ptr::write(self.ptr.add(index), last) };
        }
        self.len -= 1;
        removed
    }

    /// Removes and returns the element at `index`, shifting later elements left.
    ///
    /// Preserves order. O(n).
    ///
    /// # Panics
    ///
    /// Panics if `index >= self.len()`.
    #[inline]
    #[must_use]
    pub fn remove(&mut self, index: usize) -> T {
        assert!(
            index < self.len,
            "remove: index {index} >= len {}",
            self.len
        );
        // SAFETY: `index < len` so the element is initialized; `T: Copy`.
        let removed = unsafe { core::ptr::read(self.ptr.add(index)) };
        let tail = self.len - index - 1;
        if tail > 0 {
            // SAFETY: source `[index+1, len)` and destination `[index, len-1)` may
            // overlap, so we use `copy` (memmove semantics), not
            // `copy_nonoverlapping`.
            unsafe {
                core::ptr::copy(self.ptr.add(index + 1), self.ptr.add(index), tail);
            }
        }
        self.len -= 1;
        removed
    }

    // ── Insertion ────────────────────────────────────────────────────────────

    /// Inserts `value` at `index`, shifting later elements right. O(n).
    ///
    /// # Panics
    ///
    /// Panics if `index > self.len()`.
    #[inline]
    pub fn insert(&mut self, index: usize, value: T) {
        assert!(
            index <= self.len,
            "insert: index {index} > len {}",
            self.len
        );
        self.reserve(1);
        if index < self.len {
            // SAFETY: `index < len < capacity` (reserve ensured it). Source
            // `[index, len)` and destination `[index+1, len+1)` may overlap, so
            // we use `copy` (memmove).
            unsafe {
                core::ptr::copy(
                    self.ptr.add(index),
                    self.ptr.add(index + 1),
                    self.len - index,
                );
            }
        }
        // SAFETY: `ptr.add(index)` is inside the now-larger allocation.
        unsafe { core::ptr::write(self.ptr.add(index), value) };
        self.len += 1;
    }

    // ── Filtering ────────────────────────────────────────────────────────────

    /// Retains only elements satisfying `predicate`, removing others in-place.
    ///
    /// Preserves relative order. No reallocation.
    #[inline]
    pub fn retain<F: FnMut(&T) -> bool>(&mut self, mut predicate: F) {
        let mut write = 0usize;
        for read in 0..self.len {
            // SAFETY: `read < self.len` — inside the initialized region; T: Copy.
            let elem = unsafe { core::ptr::read(self.ptr.add(read)) };
            if predicate(&elem) {
                if write != read {
                    // SAFETY: `write <= read < self.len`; T: Copy.
                    unsafe { core::ptr::write(self.ptr.add(write), elem) };
                }
                write += 1;
            }
        }
        self.len = write;
    }

    // ── Partitioning ─────────────────────────────────────────────────────────

    /// Partitions the buffer in-place around a predicate.
    ///
    /// All `true` elements come before all `false` elements. Order within each
    /// group is not preserved. Returns the count of `true` elements (the pivot
    /// index). O(n), no allocation.
    #[inline]
    pub fn partition_in_place<F: FnMut(&T) -> bool>(&mut self, mut predicate: F) -> usize {
        let mut lo = 0usize;
        let mut hi = self.len;
        loop {
            while lo < hi {
                // SAFETY: `lo < hi <= self.len`.
                let elem = unsafe { core::ptr::read(self.ptr.add(lo)) };
                if predicate(&elem) {
                    lo += 1;
                } else {
                    break;
                }
            }
            while lo < hi {
                hi -= 1;
                // SAFETY: `hi < self.len`.
                let elem = unsafe { core::ptr::read(self.ptr.add(hi)) };
                if predicate(&elem) {
                    break;
                }
            }
            if lo >= hi {
                break;
            }
            // SAFETY: lo and hi are distinct valid indices.
            unsafe {
                let a = core::ptr::read(self.ptr.add(lo));
                let b = core::ptr::read(self.ptr.add(hi));
                core::ptr::write(self.ptr.add(lo), b);
                core::ptr::write(self.ptr.add(hi), a);
            }
            lo += 1;
        }
        lo
    }

    // ── Deduplication ────────────────────────────────────────────────────────

    /// Removes consecutive duplicate elements.
    ///
    /// Sort first to deduplicate globally. In-place, no allocation.
    #[inline]
    pub fn dedup(&mut self)
    where
        T: PartialEq,
    {
        self.dedup_by_key(|x| *x);
    }

    /// Removes consecutive duplicates according to a key function.
    ///
    /// Two adjacent elements `a` and `b` are considered duplicates when
    /// `key(a) == key(b)`. The first of each run is kept.
    #[inline]
    pub fn dedup_by_key<K: PartialEq, F: FnMut(&T) -> K>(&mut self, mut key: F) {
        if self.len < 2 {
            return;
        }
        let mut write = 1usize;
        for read in 1..self.len {
            // SAFETY: `read` and `write - 1` are both `< self.len`; T: Copy.
            let elem = unsafe { core::ptr::read(self.ptr.add(read)) };
            let prev = unsafe { core::ptr::read(self.ptr.add(write - 1)) };
            if key(&elem) != key(&prev) {
                if write != read {
                    unsafe { core::ptr::write(self.ptr.add(write), elem) };
                }
                write += 1;
            }
        }
        self.len = write;
    }

    // ── Bulk transfer ────────────────────────────────────────────────────────

    /// Moves all elements of `other` into `self`, leaving `other` empty.
    #[inline]
    pub fn append(&mut self, other: &mut Self) {
        if !other.is_empty() {
            self.extend_from_slice(other.as_slice());
            other.len = 0;
        }
    }

    // ── Splitting ────────────────────────────────────────────────────────────

    /// Splits off `[at, len)` into a new `AlignedVec`; `self` keeps `[0, at)`.
    ///
    /// # Panics
    ///
    /// Panics if `at > self.len()`.
    #[must_use]
    #[inline]
    pub fn split_off(&mut self, at: usize) -> Self {
        assert!(at <= self.len, "split_off: at {at} > len {}", self.len);
        let tail_len = self.len - at;
        // SAFETY: `[at, at + tail_len)` is within the initialized region; T: Copy.
        let tail =
            Self::from_slice(unsafe { core::slice::from_raw_parts(self.ptr.add(at), tail_len) });
        self.len = at;
        tail
    }

    // ── Capacity management ──────────────────────────────────────────────────

    /// Shrinks the capacity to `len()` if possible.
    ///
    /// Best-effort: on allocator refusal the buffer remains valid unchanged.
    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.shrink_to(self.len);
    }

    // ── Uninitialised access ─────────────────────────────────────────────────

    /// Returns a raw pointer to the uninitialized spare capacity `[len, capacity)`.
    ///
    /// The caller must initialize every element in the returned slice before
    /// calling `set_len_unchecked` to extend the initialized prefix.
    #[inline]
    pub fn spare_capacity_mut(&mut self) -> *mut [T] {
        let spare_len = self.capacity - self.len;
        // SAFETY: `self.ptr.add(self.len)` is the first byte past the
        // initialized region, inside the allocation (`capacity >= len`).
        unsafe { core::ptr::slice_from_raw_parts_mut(self.ptr.add(self.len), spare_len) }
    }

    // ── Construction helpers ─────────────────────────────────────────────────

    /// Creates a buffer of `len` elements where element `i` is produced by
    /// `f(i)`. Pre-allocates upfront; no intermediate iterator.
    #[must_use]
    #[inline]
    pub fn from_fn<F: FnMut(usize) -> T>(len: usize, mut f: F) -> Self {
        let mut buf = Self::with_capacity(len);
        for i in 0..len {
            buf.push(f(i));
        }
        buf
    }

    /// Overwrites all `len()` initialized elements with `value`.
    #[inline]
    pub fn fill(&mut self, value: T) {
        // SAFETY: `[0, self.len)` is initialized; T: Copy overwrites safely.
        unsafe {
            for i in 0..self.len {
                core::ptr::write(self.ptr.add(i), value);
            }
        }
    }

    /// Resets all initialized elements to the all-zero bit pattern.
    ///
    /// Equivalent to `fill` with the zero value but uses a single
    /// `write_bytes(0)` call — faster than iterating when the size is large.
    ///
    /// Requires the all-zero bit pattern to be a valid value of `T`, which
    /// is guaranteed by the [`ScratchElement`] invariant.
    #[inline]
    pub fn zero_fill(&mut self) {
        if self.len == 0 {
            return;
        }
        // SAFETY: `[0, self.len)` is within the allocation; all-zero is a
        // valid bit pattern for every `ScratchElement` type by invariant.
        unsafe {
            core::ptr::write_bytes(self.ptr, 0, self.len);
        }
    }

    /// Copies a slice of exactly `len()` elements into the buffer.
    ///
    /// Panics if `src.len() != self.len()`.  Equivalent to
    /// `self.as_mut_slice().copy_from_slice(src)` but named for discoverability.
    #[inline]
    pub fn copy_from_slice(&mut self, src: &[T]) {
        self.as_mut_slice().copy_from_slice(src);
    }

    // ── Drain ────────────────────────────────────────────────────────────────

    /// Removes elements in `start..end`, yields them by value, then shifts
    /// later elements left to fill the gap.
    ///
    /// If the iterator is dropped before being fully consumed, remaining
    /// elements in the range are still removed.
    ///
    /// # Panics
    ///
    /// Panics if `start > end` or `end > self.len()`.
    #[inline]
    #[must_use]
    pub fn drain(&mut self, start: usize, end: usize) -> Drain<'_, T> {
        assert!(start <= end, "drain: start > end");
        assert!(end <= self.len, "drain: end > len");
        Drain {
            buf: self,
            start,
            end,
            current: start,
        }
    }

    // ── Bulk operations ──────────────────────────────────────────────────────

    /// Concatenates two slices into a new `AlignedVec`, copying both.
    ///
    /// Equivalent to `AlignedVec::from_slice(a)` + `extend_from_slice(b)`.
    #[inline]
    #[must_use]
    pub fn concat(a: &[T], b: &[T]) -> Self {
        let mut v = Self::with_capacity(a.len() + b.len());
        v.extend_from_slice(a);
        v.extend_from_slice(b);
        v
    }

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
}

// ── Drain iterator ───────────────────────────────────────────────────────────

/// A draining iterator returned by [`AlignedVec::drain`].
///
/// Yields elements in `[start, end)` by value and, on drop, shifts the
/// tail left to close the gap.
pub struct Drain<'a, T: ScratchElement> {
    buf: &'a mut AlignedVec<T>,
    start: usize,
    end: usize,
    current: usize,
}

impl<T: ScratchElement> Iterator for Drain<'_, T> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        if self.current < self.end {
            // SAFETY: `current < end <= buf.len()` — initialized; T: Copy.
            let val = unsafe { core::ptr::read(self.buf.ptr.add(self.current)) };
            self.current += 1;
            Some(val)
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.end - self.current;
        (remaining, Some(remaining))
    }
}

impl<T: ScratchElement> ExactSizeIterator for Drain<'_, T> {}

impl<T: ScratchElement> Drop for Drain<'_, T> {
    fn drop(&mut self) {
        let drain_len = self.end - self.start;
        if drain_len == 0 {
            return;
        }
        let tail_len = self.buf.len - self.end;
        if tail_len > 0 {
            // SAFETY: `[end, end + tail_len)` → `[start, start + tail_len)`;
            // may overlap (when drain_len > 0), so we use `copy` (memmove).
            unsafe {
                core::ptr::copy(
                    self.buf.ptr.add(self.end),
                    self.buf.ptr.add(self.start),
                    tail_len,
                );
            }
        }
        self.buf.len -= drain_len;
    }
}
