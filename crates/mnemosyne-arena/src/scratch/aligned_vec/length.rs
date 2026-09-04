//! Operations that change an [`AlignedVec`]'s length.
//!
//! Every one of these goes through the storage module's `reserve`, so the
//! growth policy — double the capacity — lives in one place and these are
//! the callers of it.

use super::AlignedVec;
use super::ScratchElement;

impl<T: ScratchElement> AlignedVec<T> {
    /// Creates an `AlignedVec` of exactly `len` zero-initialized elements.
    ///
    /// Equivalent to `with_capacity(len)` followed by `ensure_len(len)`, but
    /// expressed as a single constructor. All elements are zero (valid per the
    /// [`ScratchElement`] invariant).
    #[inline]
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
            let new_cap = min_len.max(self.capacity.saturating_mul(2));
            self.grow_to(new_cap);
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
    pub fn filled(len: usize, value: T) -> Self {
        let mut buffer = Self::with_capacity(len);
        buffer.resize(len, value);
        buffer
    }

    /// Creates a buffer holding a copy of `slice`.
    #[inline]
    pub fn from_slice(slice: &[T]) -> Self {
        let mut buffer = Self::with_capacity(slice.len());
        buffer.extend_from_slice(slice);
        buffer
    }

    /// Appends one element, growing the allocation if it is full.
    ///
    /// Amortized O(1): a growth doubles the capacity, so `n` pushes onto an
    /// empty buffer perform O(log n) reallocations and O(n) element writes.
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
    pub fn swap_remove(&mut self, index: usize) -> T {
        assert!(index < self.len, "swap_remove: index {index} >= len {}", self.len);
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
    pub fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len, "remove: index {index} >= len {}", self.len);
        // SAFETY: `index < len` so the element is initialized; `T: Copy`.
        let removed = unsafe { core::ptr::read(self.ptr.add(index)) };
        let tail = self.len - index - 1;
        if tail > 0 {
            // SAFETY: source `[index+1, len)` and destination `[index, len-1)` may
            // overlap, so we use `copy` (memmove semantics), not
            // `copy_nonoverlapping`.
            unsafe {
                core::ptr::copy(
                    self.ptr.add(index + 1),
                    self.ptr.add(index),
                    tail,
                );
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
        assert!(index <= self.len, "insert: index {index} > len {}", self.len);
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
        let tail = Self::from_slice(unsafe {
            core::slice::from_raw_parts(self.ptr.add(at), tail_len)
        });
        self.len = at;
        tail
    }

    // ── Capacity management ──────────────────────────────────────────────────

    /// Shrinks the capacity to `len()` if possible.
    ///
    /// Best-effort: on allocator refusal the buffer remains valid unchanged.
    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.shrink_to(0);
    }

    /// Shrinks the capacity to `max(len(), min_capacity)` if possible.
    #[inline]
    pub fn shrink_to(&mut self, min_capacity: usize) {
        let target = self.len.max(min_capacity);
        if self.capacity <= target {
            return;
        }
        if target == 0 {
            if self.capacity > 0 {
                let layout = Self::layout_for(self.capacity);
                // SAFETY: `capacity > 0` so `self.ptr` is a live allocation
                // matching `layout`. After dealloc both len and capacity are 0.
                unsafe { alloc::alloc::dealloc(self.ptr as *mut u8, layout) };
                self.ptr = core::ptr::NonNull::dangling().as_ptr();
                self.capacity = 0;
            }
            return;
        }
        let old_layout = Self::layout_for(self.capacity);
        let new_layout = Self::layout_for(target);
        // SAFETY: `self.ptr` was allocated with `old_layout`; the alignment is
        // identical (layout_for uses the same expression). `new_layout.size()`
        // is non-zero (layout_for clamps to >= 1). Null result means the
        // allocator declined; we leave `self` unchanged.
        let new_ptr = unsafe {
            alloc::alloc::realloc(self.ptr as *mut u8, old_layout, new_layout.size()) as *mut T
        };
        if !new_ptr.is_null() {
            self.ptr = new_ptr;
            self.capacity = target;
        }
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
    pub fn drain(&mut self, start: usize, end: usize) -> Drain<'_, T> {
        assert!(start <= end, "drain: start > end");
        assert!(end <= self.len, "drain: end > len");
        Drain { buf: self, start, end, current: start }
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
