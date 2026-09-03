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
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: same validity argument as `as_mut_slice` — `[0, self.len)` is
        // initialized POD `T`. `&self` precludes concurrent mutation for the
        // slice's lifetime.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Number of initialized elements: the prefix `as_slice` exposes.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no element is initialized (`len() == 0`).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Elements the current allocation can hold before `ensure_len` must
    /// reallocate; never less than `len()`.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

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

    /// Returns a raw pointer to the start of the initialized slice.
    ///
    /// Equivalent to `self.as_slice().as_ptr()`. Safe to call on shared
    /// references; the pointer is valid for `len()` elements.
    #[inline]
    pub fn as_ptr(&self) -> *const T {
        self.ptr
    }

    /// Zero-copy view of the initialized elements as raw bytes.
    ///
    /// Available with `features = ["bytemuck"]` because this method requires
    /// `T: bytemuck::Pod` — the guarantee that no byte in the element's
    /// representation is uninitialized (padding bytes are not Pod-safe).
    ///
    /// The result length is `self.len() * size_of::<T>()`.
    #[cfg(feature = "bytemuck")]
    #[inline]
    pub fn as_bytes(&self) -> &[u8]
    where
        T: bytemuck::Pod,
    {
        bytemuck::cast_slice(self.as_slice())
    }

    /// Zero-copy mutable view of the initialized elements as raw bytes.
    ///
    /// See [`as_bytes`][Self::as_bytes] for the requirements and the
    /// relationship between the returned slice length and `len()`.
    #[cfg(feature = "bytemuck")]
    #[inline]
    pub fn as_bytes_mut(&mut self) -> &mut [u8]
    where
        T: bytemuck::Pod,
    {
        bytemuck::cast_slice_mut(self.as_mut_slice())
    }

    /// Zero-copy reinterpretation of the initialized elements as a slice of
    /// a different type `U`.
    ///
    /// Available with `features = ["bytemuck"]`. Both `T` and `U` must be
    /// `bytemuck::Pod`. The call panics when `size_of::<T>() * len()` is not
    /// a multiple of `size_of::<U>()` — the same contract as
    /// `bytemuck::cast_slice`.
    ///
    /// # Use cases
    ///
    /// - View `AlignedVec<Complex32>` (interleaved re/im as f32 pairs) as
    ///   `&[f32]` for partial in-place transforms or GPU upload.
    /// - View `AlignedVec<u32>` GPU index data as `&[u8]` for zero-copy
    ///   DMA staging, without an intermediate `Vec<u8>` copy.
    #[cfg(feature = "bytemuck")]
    #[inline]
    pub fn cast_slice<U: bytemuck::Pod>(&self) -> &[U]
    where
        T: bytemuck::Pod,
    {
        bytemuck::cast_slice(self.as_slice())
    }

    /// Zero-copy mutable reinterpretation of the initialized elements as `U`.
    ///
    /// See [`cast_slice`][Self::cast_slice] for requirements and panics.
    #[cfg(feature = "bytemuck")]
    #[inline]
    pub fn cast_slice_mut<U: bytemuck::Pod>(&mut self) -> &mut [U]
    where
        T: bytemuck::Pod,
    {
        bytemuck::cast_slice_mut(self.as_mut_slice())
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
