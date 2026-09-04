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
