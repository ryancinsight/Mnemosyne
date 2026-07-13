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

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
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

    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.ptr
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
