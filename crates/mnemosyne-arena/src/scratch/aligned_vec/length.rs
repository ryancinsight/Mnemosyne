//! Operations that change an [`AlignedVec`]'s length or content.
//!
//! Length-changing operations use the storage module's doubling growth policy,
//! so scratch reuse remains amortized while quiescent release handles retention.
//!
//! Read-only queries, search, sort, and in-place reordering live in
//! [`super::query`] so each module has a single responsibility.

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
    ///
    /// Uses geometric (doubling) growth so repeated calls are amortized.
    /// When exact capacity is required (e.g., the bounded scratch path where
    /// a provision bounds the retained size), use
    /// [`ensure_len_exact`][Self::ensure_len_exact] instead.
    #[inline]
    pub fn ensure_len(&mut self, min_len: usize) {
        if min_len <= self.len {
            return;
        }
        if min_len > self.capacity {
            self.grow_geometric(min_len);
        }
        // SAFETY: capacity >= min_len after grow; `[self.len, min_len)` is
        // inside the allocation; all-zero is valid for every `ScratchElement`.
        unsafe { self.extend_with_zeros(min_len) };
    }

    /// Like [`ensure_len`][Self::ensure_len] but grows to exactly `min_len`,
    /// never more. Use when a provision bounds retained capacity so a
    /// subsequent `shrink_to` is a no-op.
    #[inline]
    pub(crate) fn ensure_len_exact(&mut self, min_len: usize) {
        if min_len <= self.len {
            return;
        }
        if min_len > self.capacity {
            self.grow_to(min_len);
        }
        // SAFETY: capacity == min_len after grow_to; same zero-validity.
        unsafe { self.extend_with_zeros(min_len) };
    }

    /// Zeros the range `[self.len, new_len)` and advances `self.len`.
    ///
    /// # Safety
    ///
    /// `capacity >= new_len` and `new_len >= self.len`. All-zero must be a
    /// valid bit pattern for `T` (guaranteed by `ScratchElement`).
    #[inline(always)]
    unsafe fn extend_with_zeros(&mut self, new_len: usize) {
        // SAFETY: the caller guarantees capacity and zero-validity.
        unsafe {
            core::ptr::write_bytes(self.ptr.add(self.len), 0, new_len - self.len);
        }
        self.len = new_len;
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
}

// ── Drain iterator ───────────────────────────────────────────────────────────

/// A draining iterator returned by [`AlignedVec::drain`].
///
/// Yields elements in `[start, end)` by value and, on drop, shifts the
/// tail left to close the gap.
pub struct Drain<'a, T: ScratchElement> {
    pub(super) buf: &'a mut AlignedVec<T>,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) current: usize,
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
