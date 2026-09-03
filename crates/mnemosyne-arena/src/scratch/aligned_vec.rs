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
