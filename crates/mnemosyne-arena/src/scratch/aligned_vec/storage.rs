//! The buffer itself: its layout, how it is created, and how it grows.
//!
//! The operations that change its length, the byte views over it, and the
//! traits it implements each live beside this. The growth helpers are
//! `pub(super)` so those siblings reach them through the parent.

use super::ScratchElement;
use alloc::vec::Vec;
use core::marker::PhantomData;

/// A growable buffer with guaranteed byte alignment for SIMD operations.
pub struct AlignedVec<T: ScratchElement> {
    pub(super) ptr: *mut T,
    pub(super) len: usize,
    pub(super) capacity: usize,
    pub(super) _phantom: PhantomData<T>,
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

    /// Grows the allocation so `additional` more elements fit without another
    /// reallocation. Leaves `len` and the buffer contents alone.
    #[inline]
    pub(super) fn reserve(&mut self, additional: usize) {
        let needed = self.len.saturating_add(additional);
        if needed > self.capacity {
            self.grow_geometric(needed);
        }
    }

    /// Sets the initialized length without writing the newly exposed range.
    ///
    /// # Safety
    ///
    /// `new_len` must not exceed [`Self::capacity`], and every element in the
    /// newly exposed range must be initialized before a safe read observes it.
    #[inline]
    pub unsafe fn set_len_unchecked(&mut self, new_len: usize) {
        debug_assert!(
            new_len <= self.capacity,
            "set_len_unchecked: {new_len} > capacity {}",
            self.capacity
        );
        self.len = new_len;
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

    /// Returns the pointer range `as_ptr()..as_ptr().add(len())` as a
    /// `core::ops::Range<*const T>`.
    ///
    /// Useful for bounds checks and FFI handoffs that expect a pointer pair
    /// rather than a fat pointer.
    #[inline]
    pub fn as_ptr_range(&self) -> core::ops::Range<*const T> {
        // SAFETY: `self.ptr.add(self.len)` stays within (or one-past-end of)
        // the allocation — `capacity >= len`.
        let start = self.ptr as *const T;
        // SAFETY: same argument as `as_slice`.
        let end = unsafe { start.add(self.len) };
        start..end
    }

    /// Mutable counterpart of [`as_ptr_range`][Self::as_ptr_range].
    ///
    /// Returns `as_mut_ptr()..as_mut_ptr().add(len())`.
    #[inline]
    pub fn as_mut_ptr_range(&mut self) -> core::ops::Range<*mut T> {
        let start = self.ptr;
        // SAFETY: `self.ptr.add(self.len)` stays within the allocation.
        let end = unsafe { start.add(self.len) };
        start..end
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
    pub(super) fn grow_to(&mut self, new_capacity: usize) {
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

    /// Applies the shared geometric growth policy to a required capacity.
    ///
    /// A virgin allocation is exact because `capacity` is zero; an existing
    /// allocation doubles unless the request is larger. The policy keeps
    /// reallocation and copy traffic amortized, while [`shrink_to_capacity`]
    /// and the scratch-pool release path bound idle retention separately.
    #[cold]
    #[inline(never)]
    pub(super) fn grow_geometric(&mut self, needed: usize) {
        self.grow_to(needed.max(self.capacity.saturating_mul(2)));
    }

    /// Reallocates the buffer down to exactly `new_capacity` elements,
    /// returning the surplus to the allocator.
    ///
    /// This is the counterpart of [`grow_to`](Self::grow_to) and the only
    /// operation on this type that gives memory back. It never grows: when
    /// `new_capacity >= self.capacity()` it is a no-op. `len` is clamped to
    /// the new capacity, so the type's `len <= capacity` invariant survives;
    /// the retained elements keep their values.
    ///
    /// Callers that need the released region to read as fresh later must
    /// re-zero it themselves; shrinking only preserves the initialized prefix.
    ///
    /// # Panics
    ///
    /// Panics if the allocator reports failure for the smaller layout.
    fn shrink_to_capacity(&mut self, new_capacity: usize) {
        if new_capacity >= self.capacity {
            return;
        }
        if new_capacity == 0 {
            if self.capacity > 0 {
                // Let `Drop` return the old allocation, then install the
                // dangling sentinel: `*self =` drops the old value first, so a
                // manual `dealloc` here would free the same block twice.
                *self = Self::dangling();
            }
            return;
        }
        let old_layout = Self::layout_for(self.capacity);
        let new_layout = Self::layout_for(new_capacity);
        // SAFETY: `self.ptr` was allocated by this same allocator with
        // `old_layout` (`capacity != 0` is guaranteed by the early return
        // above), and both layouts share the alignment of `T`.
        // `new_layout.size()` is non-zero (`layout_for` clamps to >= 1), so
        // this shrinks rather than frees; the result is null-checked before
        // it replaces `self.ptr`.
        let new_ptr =
            unsafe { alloc::alloc::realloc(self.ptr as *mut u8, old_layout, new_layout.size()) }
                as *mut T;
        if new_ptr.is_null() {
            alloc::alloc::handle_alloc_error(new_layout);
        }
        self.ptr = new_ptr;
        self.capacity = new_capacity;
        if self.len > self.capacity {
            self.len = self.capacity;
        }
    }

    /// Reallocates the buffer down to exactly `new_capacity` elements,
    /// returning the surplus to the allocator.
    ///
    /// This is the public length-oriented entry point for the capacity
    /// reduction operation.
    #[inline]
    pub fn shrink_to(&mut self, new_capacity: usize) {
        self.shrink_to_capacity(new_capacity);
    }

    #[inline]
    pub(super) fn layout_for(capacity: usize) -> core::alloc::Layout {
        let elem_size = core::mem::size_of::<T>();
        let size = capacity.saturating_mul(elem_size).max(1);
        let align = T::ALIGN_BYTES.max(elem_size);
        core::alloc::Layout::from_size_align(size, align).expect("AlignedVec: invalid layout")
    }
}
