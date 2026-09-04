//! Aligned, grow-only scratch vector whose newly grown range is zeroed so a
//! reused buffer never exposes stale data.

use alloc::vec::Vec;
use core::marker::PhantomData;

use super::element::ScratchElement;

/// A growable buffer with guaranteed byte alignment for SIMD operations.
pub struct AlignedVec<T: ScratchElement> {
    ptr: *mut T,
    len: usize,
    capacity: usize,
    _phantom: PhantomData<T>,
}

impl<T: ScratchElement> AlignedVec<T> {
    /// Creates a dangling sentinel with zero capacity.
    #[inline]
    #[must_use]
    pub const fn dangling() -> Self {
        Self {
            ptr: core::ptr::NonNull::dangling().as_ptr(),
            len: 0,
            capacity: 0,
            _phantom: PhantomData,
        }
    }

    /// Creates a new `AlignedVec` with the given initial capacity.
    #[inline]
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        if capacity == 0 {
            return Self::dangling();
        }
        let layout = Self::layout_for(capacity);
        // SAFETY: `capacity != 0` here and `layout_for` clamps the byte size to
        // at least 1, so `layout` is a valid non-zero-size layout for the global
        // allocator. The returned pointer is null-checked immediately below
        // before it is stored or dereferenced.
        let ptr = unsafe { alloc::alloc::alloc(layout) } as *mut T;
        if ptr.is_null() {
            alloc::alloc::handle_alloc_error(layout);
        }
        Self {
            ptr,
            len: 0,
            capacity,
            _phantom: PhantomData,
        }
    }

    /// Returns a mutable slice of the initialized elements.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: `self.ptr` addresses an allocation of `self.capacity >= self.len`
        // elements, and `[0, self.len)` is fully initialized (`with_capacity`
        // starts `len` at 0; `ensure_len` zero-initializes every newly exposed
        // element before advancing `len`). `T: ScratchElement` is `Copy`/POD, so
        // the initialized bytes form valid `T` values. `&mut self` proves
        // exclusive access for the slice's lifetime.
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// Returns a shared slice of the initialized elements.
    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: same validity argument as `as_mut_slice` — `[0, self.len)` is
        // initialized POD `T`. `&self` precludes concurrent mutation for the
        // slice's lifetime.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Number of initialized elements: the prefix `as_slice` exposes.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no element is initialized (`len() == 0`).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Sets the length to zero, retaining the allocation for reuse.
    ///
    /// Equivalent to `self.len = 0`; `ScratchElement` elements are `Copy`
    /// POD so no destructors need to run when the logical slice shrinks.
    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Shortens the buffer to `new_len` elements.
    ///
    /// If `new_len >= self.len()` this is a no-op. Retains the allocation.
    /// `ScratchElement` is `Copy` POD — no destructors run.
    #[inline]
    pub fn truncate(&mut self, new_len: usize) {
        if new_len < self.len {
            self.len = new_len;
        }
    }

    /// Retains only the elements satisfying `predicate`, removing the rest
    /// in-place.
    ///
    /// Maintains relative element order. Allocation is never shrunk.
    #[inline]
    pub fn retain<F>(&mut self, mut predicate: F)
    where
        F: FnMut(&T) -> bool,
    {
        let mut write = 0usize;
        for read in 0..self.len {
            // SAFETY: `read < self.len` so the index is inside the initialized
            // region; the read copies a `Copy` POD element.
            let elem = unsafe { core::ptr::read(self.ptr.add(read)) };
            if predicate(&elem) {
                if write != read {
                    // SAFETY: both `write` and `read` are `< self.len` (write <= read),
                    // so both offsets are inside the allocation. `T: Copy` — no
                    // aliasing hazard from the read above to the write below.
                    unsafe { core::ptr::write(self.ptr.add(write), elem) };
                }
                write += 1;
            }
        }
        self.len = write;
    }

    /// Removes consecutive duplicate elements.
    ///
    /// Requires `T: PartialEq`. If the buffer is not sorted, only adjacent
    /// duplicates are removed; call `sort_unstable()` (via `Deref` to `[T]`)
    /// first to deduplicate globally.
    ///
    /// This is the in-place counterpart of `Vec::dedup()`, which is not
    /// available on slices alone.
    #[inline]
    pub fn dedup(&mut self)
    where
        T: PartialEq,
    {
        self.dedup_by_key(|x| *x);
    }

    /// Removes consecutive duplicates according to a key function.
    ///
    /// Two elements `a` and `b` are considered duplicates when
    /// `key(a) == key(b)`. The *first* of each run is kept.
    #[inline]
    pub fn dedup_by_key<K: PartialEq, F: FnMut(&T) -> K>(&mut self, mut key: F) {
        if self.len < 2 {
            return;
        }
        let mut write = 1usize;
        for read in 1..self.len {
            // SAFETY: `read` and `write` are both in `[0, self.len)` since
            // `write <= read < self.len`; `T: Copy` — reads are bitwise copies.
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

    /// Removes the elements in the given range from the buffer, returns them
    /// as an iterator, and shifts the elements after the range to fill the gap.
    ///
    /// The returned iterator yields elements in front-to-back order.  If the
    /// iterator is dropped before being fully consumed, any unread elements are
    /// silently discarded (they have already been removed from the buffer).
    ///
    /// # Panics
    ///
    /// Panics if `start > end` or `end > self.len()`.
    #[inline]
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

    /// Elements the current allocation can hold before `ensure_len` must
    /// reallocate; never less than `len()`.
    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

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

    /// Creates a buffer of exactly `len` elements initialized by `f(index)`.
    ///
    /// The closure receives the zero-based index of the element being written.
    /// Equivalent to `(0..len).map(f).collect::<AlignedVec<T>>()` but without
    /// iterator overhead — the backing allocation is made upfront.
    ///
    /// # Example
    ///
    /// ```
    /// # use mnemosyne_arena::AlignedVec;
    /// let v = AlignedVec::<u32>::from_fn(4, |i| i as u32 * 2);
    /// assert_eq!(v.as_slice(), &[0, 2, 4, 6]);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_fn<F: FnMut(usize) -> T>(len: usize, mut f: F) -> Self {
        let mut buf = Self::with_capacity(len);
        for i in 0..len {
            buf.push(f(i));
        }
        buf
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

    /// Grows the allocation so `additional` more elements fit without another
    /// reallocation. Leaves `len` and the buffer contents alone.
    #[inline]
    fn reserve(&mut self, additional: usize) {
        let needed = self.len.saturating_add(additional);
        if needed > self.capacity {
            self.grow_to(needed.max(self.capacity.saturating_mul(2)));
        }
    }

    /// Raw pointer to the start of the aligned allocation. Valid for
    /// `capacity()` elements, of which the first `len()` are initialized;
    /// writes past `len()` do not extend the initialized prefix.
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.ptr
    }

    /// Returns a shared pointer to the first element (same as
    /// `self.as_slice().as_ptr()`).
    #[inline]
    pub fn as_ptr(&self) -> *const T {
        self.ptr
    }

    /// Overwrites all `len()` initialized elements with `value`.
    ///
    /// Does not grow the buffer; only the `[0, len())` prefix is touched.
    #[inline]
    pub fn fill(&mut self, value: T) {
        // SAFETY: `[0, self.len)` is fully initialized POD `T`; overwriting
        // each element with a `Copy` value needs no destructor and stays inside
        // the allocation.
        unsafe {
            for i in 0..self.len {
                core::ptr::write(self.ptr.add(i), value);
            }
        }
    }

    /// Ensures length ≥ `min_len` then overwrites the whole `[0, min_len)`
    /// prefix with `value`.
    ///
    /// Equivalent to `resize(min_len, value)` followed by `fill(value)` but
    /// avoids a redundant second pass over elements that `resize` already wrote.
    #[inline]
    pub fn resize_fill(&mut self, min_len: usize, value: T) {
        self.resize(min_len, value);
        // `resize` already wrote `value` into any newly-added range; overwrite
        // the pre-existing prefix so the entire `[0, min_len)` slice is uniform.
        if min_len <= self.len {
            self.fill(value);
        }
    }

    /// Sets the logical length to `new_len` without initializing the extended
    /// range or running any destructors on the shrunk tail.
    ///
    /// # Safety
    ///
    /// * `new_len <= self.capacity()`.
    /// * If `new_len > self.len()` the caller must ensure every element in
    ///   `[old_len, new_len)` is initialized before the next call that reads
    ///   through `as_slice()` or `Deref`. Violating either condition is UB.
    #[inline]
    pub unsafe fn set_len_unchecked(&mut self, new_len: usize) {
        debug_assert!(
            new_len <= self.capacity,
            "set_len_unchecked: new_len={new_len} exceeds capacity={}",
            self.capacity
        );
        self.len = new_len;
    }

    /// Consumes self and returns a `Vec<T>` with the initialized data.
    #[inline]
    pub fn into_vec(self) -> Vec<T> {
        let mut v = Vec::with_capacity(self.len);
        // SAFETY: `v` was reserved with capacity `self.len`, so its buffer holds
        // `self.len` elements and is a distinct allocation that cannot overlap
        // `self.ptr`. `[0, self.len)` of `self.ptr` is initialized POD `T`, and
        // `T: Copy` makes the bytewise copy valid. `set_len(self.len)` matches
        // exactly the number of elements copied. The source retains ownership
        // of its distinct allocation and is released by its normal `Drop`
        // after this method returns.
        unsafe {
            core::ptr::copy_nonoverlapping(self.ptr, v.as_mut_ptr(), self.len);
            v.set_len(self.len);
        }
        v
    }

    /// Shrinks the capacity as close to `len()` as possible.
    ///
    /// If the buffer is empty (`len() == 0`) the allocation is freed. If
    /// `capacity == len` already, nothing happens. Otherwise a `realloc`
    /// down to exactly `len` elements is attempted; if the allocator declines
    /// the resize the capacity is unchanged (shrink is best-effort, never
    /// panics).
    #[inline]
    pub fn shrink_to_fit(&mut self) {
        self.shrink_to(0);
    }

    /// Shrinks the capacity to at most `max(len(), min_capacity)` elements.
    ///
    /// No reallocation happens if the current capacity already satisfies the
    /// constraint. The `realloc` is best-effort — on failure the capacity
    /// remains unchanged and the buffer is still valid.
    #[inline]
    pub fn shrink_to(&mut self, min_capacity: usize) {
        let target = self.len.max(min_capacity);
        if self.capacity <= target {
            return;
        }
        if target == 0 {
            if self.capacity > 0 {
                let layout = Self::layout_for(self.capacity);
                // SAFETY: `capacity > 0` so `self.ptr` is a live aligned
                // allocation matching `layout`. After dealloc, `len` and
                // `capacity` are both set to 0 so no further reads happen.
                unsafe { alloc::alloc::dealloc(self.ptr as *mut u8, layout) };
                self.ptr = core::ptr::NonNull::dangling().as_ptr();
                self.capacity = 0;
            }
            return;
        }
        let old_layout = Self::layout_for(self.capacity);
        let new_layout = Self::layout_for(target);
        // SAFETY: `self.ptr` was allocated with `old_layout`; `new_layout`
        // shares alignment (layout_for uses the same alignment expression).
        // `new_layout.size()` is non-zero (layout_for clamps to >= 1). The
        // result is null-checked before committing.
        let new_ptr = unsafe {
            alloc::alloc::realloc(self.ptr as *mut u8, old_layout, new_layout.size()) as *mut T
        };
        if !new_ptr.is_null() {
            self.ptr = new_ptr;
            self.capacity = target;
        }
        // On null, leave self unchanged — shrink is advisory.
    }

    /// Removes the element at `index` by swapping it with the last element
    /// and truncating, then returns the removed value.
    ///
    /// Does not preserve element order. O(1).
    ///
    /// # Panics
    ///
    /// Panics if `index >= self.len()`.
    #[inline]
    pub fn swap_remove(&mut self, index: usize) -> T {
        assert!(index < self.len, "swap_remove: index out of bounds");
        // SAFETY: `index < len` and `len - 1 < len`, so both indices are
        // inside the initialized region; T: Copy — bitwise reads are sound.
        let last = unsafe { core::ptr::read(self.ptr.add(self.len - 1)) };
        let removed = unsafe { core::ptr::read(self.ptr.add(index)) };
        if index != self.len - 1 {
            // SAFETY: `index < len`, so writing `last` at `index` stays in bounds.
            unsafe { core::ptr::write(self.ptr.add(index), last) };
        }
        self.len -= 1;
        removed
    }

    /// Moves all elements of `other` into `self`, leaving `other` empty.
    ///
    /// Equivalent to `self.extend_from_slice(other.as_slice()); other.clear()`.
    /// Uses `copy_nonoverlapping` for the bulk copy.
    #[inline]
    pub fn append(&mut self, other: &mut Self) {
        if other.is_empty() {
            return;
        }
        self.extend_from_slice(other.as_slice());
        other.len = 0;
    }

    /// Splits the buffer into two at the given index.
    ///
    /// Returns a new `AlignedVec` containing elements `[at, len)`.
    /// `self` retains elements `[0, at)` and is shortened accordingly.
    ///
    /// # Panics
    ///
    /// Panics if `at > self.len()`.
    #[must_use]
    #[inline]
    pub fn split_off(&mut self, at: usize) -> Self {
        assert!(at <= self.len, "split_off: at > len");
        let tail_len = self.len - at;
        // SAFETY: `at .. at + tail_len` is within initialized bounds; T: Copy.
        let tail = Self::from_slice(unsafe {
            core::slice::from_raw_parts(self.ptr.add(at), tail_len)
        });
        self.len = at;
        tail
    }

    /// Returns a mutable pointer covering the spare (uninitialized) capacity
    /// `[len, capacity)`.
    ///
    /// Callers must initialize every element in the returned slice before
    /// calling `set_len_unchecked` to extend the initialized prefix. The
    /// returned pointer is valid only until the next reallocation.
    #[inline]
    pub fn spare_capacity_mut(&mut self) -> *mut [T] {
        let spare_len = self.capacity - self.len;
        // SAFETY: `self.ptr.add(self.len)` is the first byte past the
        // initialized region and before the allocation end (`capacity` was
        // grown to `>= len`). The resulting pointer-to-slice is only a raw
        // pointer, so no reference-aliasing rules apply until the caller
        // constructs a reference from it.
        unsafe {
            core::ptr::slice_from_raw_parts_mut(self.ptr.add(self.len), spare_len)
        }
    }

    #[cold]
    #[inline(never)]
    fn grow_to(&mut self, new_capacity: usize) {
        let new_layout = Self::layout_for(new_capacity);
        let new_ptr = if self.capacity == 0 {
            // SAFETY: `new_layout` has non-zero size (`layout_for` clamps to
            // `>= 1`); the result is null-checked below before use.
            unsafe { alloc::alloc::alloc(new_layout) as *mut T }
        } else {
            let old_layout = Self::layout_for(self.capacity);
            // SAFETY: `self.ptr` was allocated by this same allocator with
            // `old_layout` (the `capacity != 0` branch), and `old_layout` and
            // `new_layout` share the same alignment because `layout_for`'s
            // alignment depends only on `T`. `new_layout.size()` is non-zero.
            // The result is null-checked below before it replaces `self.ptr`.
            unsafe {
                alloc::alloc::realloc(self.ptr as *mut u8, old_layout, new_layout.size()) as *mut T
            }
        };
        if new_ptr.is_null() {
            alloc::alloc::handle_alloc_error(new_layout);
        }
        self.ptr = new_ptr;
        self.capacity = new_capacity;
    }

    #[inline]
    fn layout_for(capacity: usize) -> core::alloc::Layout {
        let elem_size = core::mem::size_of::<T>();
        let size = capacity.saturating_mul(elem_size).max(1);
        let align = T::ALIGN_BYTES.max(elem_size);
        core::alloc::Layout::from_size_align(size, align).expect("AlignedVec: invalid layout")
    }
}

// ── Drain iterator ───────────────────────────────────────────────────────────

/// A draining iterator returned by [`AlignedVec::drain`].
///
/// Elements in `[start, end)` are yielded by value.  When the iterator is
/// dropped — whether fully consumed or not — any remaining un-yielded elements
/// in the drain range are discarded and the elements after the range are
/// shifted left to close the gap.
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
            // SAFETY: `current < end <= buf.len()`, so this element is inside
            // the initialized region; `T: Copy` — bitwise read is sound.
            let val = unsafe { core::ptr::read(self.buf.as_ptr().add(self.current)) };
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
        // Shift elements in [end, buf.len) left by `(end - start)` positions
        // to fill the drained range, then reduce the length.
        let drain_len = self.end - self.start;
        if drain_len == 0 {
            return;
        }
        let tail_len = self.buf.len - self.end;
        if tail_len > 0 {
            // SAFETY: The source range `[end, end + tail_len)` and the destination
            // range `[start, start + tail_len)` do not overlap when `end > start`
            // (the drain range is non-empty), and both lie within the initialized
            // allocation.  `T: Copy` — no destructor hazard.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.buf.ptr.add(self.end),
                    self.buf.ptr.add(self.start),
                    tail_len,
                );
            }
        }
        self.buf.len -= drain_len;
    }
}

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

impl<T: ScratchElement> Clone for AlignedVec<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self::from_slice(self.as_slice())
    }
}

impl<T: ScratchElement + PartialEq> PartialEq for AlignedVec<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: ScratchElement + Eq> Eq for AlignedVec<T> {}

impl<T: ScratchElement + PartialEq> PartialEq<[T]> for AlignedVec<T> {
    #[inline]
    fn eq(&self, other: &[T]) -> bool {
        self.as_slice() == other
    }
}

impl<T: ScratchElement + core::fmt::Debug> core::fmt::Debug for AlignedVec<T> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.as_slice().fmt(f)
    }
}

impl<T: ScratchElement + core::hash::Hash> core::hash::Hash for AlignedVec<T> {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl<T: ScratchElement> Default for AlignedVec<T> {
    #[inline]
    fn default() -> Self {
        Self::dangling()
    }
}

impl<T: ScratchElement> From<&[T]> for AlignedVec<T> {
    #[inline]
    fn from(slice: &[T]) -> Self {
        Self::from_slice(slice)
    }
}

impl<T: ScratchElement> From<alloc::vec::Vec<T>> for AlignedVec<T> {
    #[inline]
    fn from(v: alloc::vec::Vec<T>) -> Self {
        Self::from_slice(&v)
    }
}

impl<T: ScratchElement> From<AlignedVec<T>> for alloc::vec::Vec<T> {
    #[inline]
    fn from(av: AlignedVec<T>) -> Self {
        av.into_vec()
    }
}

// ── Iteration ────────────────────────────────────────────────────────────────

/// Zero-cost borrowed iterator: delegates directly to `[T]::iter()`.
impl<'a, T: ScratchElement> IntoIterator for &'a AlignedVec<T> {
    type Item = &'a T;
    type IntoIter = core::slice::Iter<'a, T>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

/// Zero-cost mutable borrowed iterator: delegates directly to `[T]::iter_mut()`.
impl<'a, T: ScratchElement> IntoIterator for &'a mut AlignedVec<T> {
    type Item = &'a mut T;
    type IntoIter = core::slice::IterMut<'a, T>;
    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.as_mut_slice().iter_mut()
    }
}

/// Owned iterator that consumes the `AlignedVec` and yields `T` by value.
///
/// Advances a position counter rather than moving the pointer, so the
/// allocation can be freed at drop time regardless of how many items were
/// consumed.
pub struct AlignedVecIntoIter<T: ScratchElement> {
    buf: AlignedVec<T>,
    pos: usize,
}

impl<T: ScratchElement> Iterator for AlignedVecIntoIter<T> {
    type Item = T;
    #[inline]
    fn next(&mut self) -> Option<T> {
        if self.pos < self.buf.len() {
            // SAFETY: `pos < len` means the element is inside the initialized
            // region; `T: Copy` makes a bitwise read safe without moving
            // ownership (the buffer owns the storage, not the logical elements).
            let val = unsafe { core::ptr::read(self.buf.as_ptr().add(self.pos)) };
            self.pos += 1;
            Some(val)
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.buf.len() - self.pos;
        (remaining, Some(remaining))
    }
}

impl<T: ScratchElement> ExactSizeIterator for AlignedVecIntoIter<T> {}

impl<T: ScratchElement> core::iter::DoubleEndedIterator for AlignedVecIntoIter<T> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        let end = self.buf.len();
        if self.pos < end {
            // Logically "pop" from the back by reducing len by 1.
            // SAFETY: `end - 1 < len` so the element is in the initialized
            // region; `T: Copy` — bitwise read is sound.
            let new_end = end - 1;
            // SAFETY: `set_len_unchecked` contract: new_end <= capacity (it's
            // the old len minus 1) and the element at `new_end` is initialized.
            unsafe { self.buf.set_len_unchecked(new_end) };
            let val = unsafe { core::ptr::read(self.buf.as_ptr().add(new_end)) };
            Some(val)
        } else {
            None
        }
    }
}

impl<T: ScratchElement> IntoIterator for AlignedVec<T> {
    type Item = T;
    type IntoIter = AlignedVecIntoIter<T>;
    #[inline]
    fn into_iter(self) -> AlignedVecIntoIter<T> {
        AlignedVecIntoIter { buf: self, pos: 0 }
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
        for item in iter {
            self.push(item);
        }
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
