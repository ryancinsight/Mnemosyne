//! Temporal aligned scratch pool for high-performance numerical workloads.
//!
//! FFT and transform workloads (e.g. Apollo) repeatedly need large, aligned
//! temporary buffers for Stockham autosort, Bluestein chirp, PFA scratch, and
//! Rader convolution. These buffers are typically allocated once, grown to the
//! maximum needed size, and reused across many transform calls.
//!
//! This module provides a [`ScratchPool`] that manages thread-local pools of
//! reusable, aligned buffers with:
//! - **Closure-based access**: `pool.with_scratch(n, |slice| ...)` guarantees
//!   RAII-style borrow depth release — no manual cleanup required.
//! - **Exact-length slices**: The closure receives exactly `n` elements.
//! - **Alignment**: Buffers are aligned to 64-byte AVX-512 cache lines.
//! - **Growth-on-demand**: Buffers grow geometrically and never shrink.
//! - **Zero-cost generics**: Monomorphized per element type, no dynamic dispatch.
//! - **Zero-copy**: Direct `&mut [T]` slice into the pool buffer.

use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::marker::PhantomData;

/// Default alignment for scratch buffers (64 bytes = one AVX-512 cache line).
pub const DEFAULT_SCRATCH_ALIGN: usize = 64;

// ---------------------------------------------------------------------------
// Sealed trait: which element types the pool can serve
// ---------------------------------------------------------------------------

mod sealed {
    pub trait ScratchElementSealed {}
}

/// Element types that the scratch pool can manage.
///
/// Implemented for `f32`, `f64`, `u8`, and (with the `num-complex` feature)
/// `num_complex::Complex32` and `num_complex::Complex64`. The trait is sealed
/// so new implementations cannot be added downstream.
pub trait ScratchElement: sealed::ScratchElementSealed + Copy + Send + Sync + 'static {
    /// Alignment in bytes required for SIMD operations on this element type.
    const ALIGN_BYTES: usize;
}

impl sealed::ScratchElementSealed for f32 {}
impl ScratchElement for f32 {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

impl sealed::ScratchElementSealed for f64 {}
impl ScratchElement for f64 {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

impl sealed::ScratchElementSealed for u8 {}
impl ScratchElement for u8 {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

#[cfg(feature = "num-complex")]
impl sealed::ScratchElementSealed for num_complex::Complex32 {}
#[cfg(feature = "num-complex")]
impl ScratchElement for num_complex::Complex32 {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

#[cfg(feature = "num-complex")]
impl sealed::ScratchElementSealed for num_complex::Complex64 {}
#[cfg(feature = "num-complex")]
impl ScratchElement for num_complex::Complex64 {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

// ---------------------------------------------------------------------------
// AlignedVec: a Vec-like container with guaranteed alignment
// ---------------------------------------------------------------------------

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
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// Returns a shared slice of the initialized elements.
    #[inline]
    pub fn as_slice(&self) -> &[T] {
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
    fn ensure_len(&mut self, min_len: usize) {
        if min_len <= self.len {
            return;
        }
        if min_len > self.capacity {
            let new_cap = min_len.max(self.capacity.saturating_mul(2));
            self.grow_to(new_cap);
        }
        // Zero only the newly added range.
        unsafe {
            let dst = self.ptr.add(self.len);
            core::ptr::write_bytes(dst, 0, min_len - self.len);
        }
        self.len = min_len;
    }

    /// Sets the logical length without zeroing. Caller must ensure elements
    /// in `[old_len, new_len)` are written before reading.
    ///
    /// # Safety
    ///
    /// `new_len` must be <= `self.capacity`. Elements in `[old_len, new_len)`
    /// are uninitialized and must be written before read.
    #[inline]
    unsafe fn set_len(&mut self, new_len: usize) {
        debug_assert!(new_len <= self.capacity);
        self.len = new_len;
    }

    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.ptr
    }

    /// Consumes self and returns a `Vec<T>` with the initialized data.
    #[inline]
    pub fn into_vec(self) -> Vec<T> {
        let mut v = Vec::with_capacity(self.len);
        unsafe {
            core::ptr::copy_nonoverlapping(self.ptr, v.as_mut_ptr(), self.len);
            v.set_len(self.len);
        }
        core::mem::forget(self);
        v
    }

    #[cold]
    #[inline(never)]
    fn grow_to(&mut self, new_capacity: usize) {
        let new_layout = Self::layout_for(new_capacity);
        let new_ptr = if self.capacity == 0 {
            unsafe { alloc::alloc::alloc(new_layout) as *mut T }
        } else {
            let old_layout = Self::layout_for(self.capacity);
            unsafe {
                alloc::alloc::realloc(self.ptr as *mut u8, old_layout, new_layout.size())
                    as *mut T
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
        core::alloc::Layout::from_size_align(size, align)
            .expect("AlignedVec: invalid layout")
    }
}

impl<T: ScratchElement> Drop for AlignedVec<T> {
    fn drop(&mut self) {
        if self.capacity > 0 {
            let layout = Self::layout_for(self.capacity);
            unsafe {
                alloc::alloc::dealloc(self.ptr as *mut u8, layout);
            }
        }
    }
}

unsafe impl<T: ScratchElement + Send> Send for AlignedVec<T> {}

// ---------------------------------------------------------------------------
// ScratchPool: closure-based, RAII-safe, aligned scratch buffer pool
// ---------------------------------------------------------------------------

/// Maximum concurrent borrows (recursive/nested calls) the pool supports.
const MAX_POOL_SLOTS: usize = 4;

/// A pool of reusable, aligned scratch buffers for a specific element type.
///
/// `Send` but **not** `Sync` — designed for `thread_local!` storage.
///
/// # Usage
///
/// ```rust,ignore
/// use mnemosyne_arena::scratch::ScratchPool;
///
/// thread_local! {
///     static POOL: ScratchPool<f64> = ScratchPool::new();
/// }
///
/// POOL.with(|pool| {
///     pool.with_scratch(1024, |scratch| {
///         // scratch: &mut [f64] of exactly 1024 elements, 64-byte aligned
///     });
/// });
/// ```
pub struct ScratchPool<T: ScratchElement> {
    slots: [UnsafeCell<AlignedVec<T>>; MAX_POOL_SLOTS],
    borrow_depth: core::cell::Cell<u8>,
}

unsafe impl<T: ScratchElement> Send for ScratchPool<T> {}

impl<T: ScratchElement> Default for ScratchPool<T> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ScratchElement> ScratchPool<T> {
    /// Creates a new empty scratch pool (zero allocation at construction).
    #[inline]
    pub const fn new() -> Self {
        Self {
            slots: [
                UnsafeCell::new(AlignedVec::dangling()),
                UnsafeCell::new(AlignedVec::dangling()),
                UnsafeCell::new(AlignedVec::dangling()),
                UnsafeCell::new(AlignedVec::dangling()),
            ],
            borrow_depth: core::cell::Cell::new(0),
        }
    }

    /// Creates a new scratch pool with pre-allocated capacity per slot.
    #[inline]
    pub fn with_slot_capacity(capacity: usize) -> Self {
        let mk = || {
            if capacity == 0 {
                AlignedVec::dangling()
            } else {
                AlignedVec::with_capacity(capacity)
            }
        };
        Self {
            slots: [
                UnsafeCell::new(mk()),
                UnsafeCell::new(mk()),
                UnsafeCell::new(mk()),
                UnsafeCell::new(mk()),
            ],
            borrow_depth: core::cell::Cell::new(0),
        }
    }

    /// Provides a mutable aligned scratch slice of **exactly** `n` elements
    /// to the closure. Borrow depth is released when the closure returns.
    ///
    /// If a pool slot is available, the closure receives a direct `&mut [T]`
    /// into the pooled buffer (zero-copy). If all slots are exhausted (nested
    /// recursive calls), a temporary buffer is allocated instead.
    #[inline]
    pub fn with_scratch<R>(&self, n: usize, f: impl FnOnce(&mut [T]) -> R) -> R {
        let depth = self.borrow_depth.get();
        if depth < MAX_POOL_SLOTS as u8 {
            self.borrow_depth.set(depth + 1);
            // SAFETY: exclusive access guaranteed by borrow_depth tracking.
            // Each nesting level gets its own slot index.
            let vec = unsafe { &mut *self.slots[depth as usize].get() };
            // Ensure the buffer is large enough. If the buffer was already
            // grown by a prior call, reuse it without re-zeroing (only newly
            // added elements are zeroed by ensure_len).
            if n > vec.len() {
                vec.ensure_len(n);
            }
            debug_assert_eq!(
                vec.as_mut_ptr() as usize % T::ALIGN_BYTES,
                0,
                "Scratch buffer not aligned to {} bytes",
                T::ALIGN_BYTES
            );
            // Return exactly `n` elements (not the full buffer).
            let slice = &mut vec.as_mut_slice()[..n];
            let result = f(slice);
            self.borrow_depth.set(depth);
            result
        } else {
            // All slots exhausted; allocate owned fallback.
            let mut owned = AlignedVec::with_capacity(n);
            // SAFETY: we just allocated `n` capacity, and will write all
            // elements through the closure before reading.
            unsafe { owned.set_len(n) };
            f(owned.as_mut_slice())
        }
    }

    /// Returns the current borrow depth (0 = fully available).
    #[inline]
    pub fn borrow_depth(&self) -> u8 {
        self.borrow_depth.get()
    }

    /// Returns the capacity of the first slot (primary buffer).
    #[inline]
    pub fn capacity(&self) -> usize {
        // SAFETY: reading capacity is safe.
        unsafe { (*self.slots[0].get()).capacity() }
    }
}

/// Default alignment constant for external consumers.
#[inline]
pub const fn default_align() -> usize {
    DEFAULT_SCRATCH_ALIGN
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn aligned_vec_capacity_and_alignment() {
        let mut v = AlignedVec::<f64>::with_capacity(256);
        v.ensure_len(256);
        assert_eq!(v.len(), 256);
        assert!(v.capacity() >= 256);
        assert_eq!(v.as_mut_ptr() as usize % DEFAULT_SCRATCH_ALIGN, 0);
    }

    #[test]
    fn aligned_vec_growth_preserves_data() {
        let mut v = AlignedVec::<f32>::with_capacity(4);
        v.ensure_len(4);
        v.as_mut_slice().copy_from_slice(&[1.0, 2.0, 3.0, 4.0]);
        v.ensure_len(8);
        assert_eq!(&v.as_slice()[..4], &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(v.len(), 8);
    }

    #[test]
    fn aligned_vec_zero_capacity_is_valid() {
        let v = AlignedVec::<f32>::dangling();
        assert_eq!(v.len(), 0);
        assert!(v.is_empty());
        assert_eq!(v.capacity(), 0);
    }

    #[test]
    fn aligned_vec_into_vec() {
        let mut v = AlignedVec::<f64>::with_capacity(4);
        v.ensure_len(4);
        v.as_mut_slice().copy_from_slice(&[1.0, 2.0, 3.0, 4.0]);
        let vec = v.into_vec();
        assert_eq!(vec, std::vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn scratch_pool_single_borrow() {
        let pool = ScratchPool::<f64>::new();
        pool.with_scratch(128, |scratch| {
            assert_eq!(scratch.len(), 128, "must return exactly n elements");
            scratch[0] = 42.0;
            assert_eq!(scratch[0], 42.0);
            assert_eq!(scratch.as_ptr() as usize % DEFAULT_SCRATCH_ALIGN, 0);
        });
        assert_eq!(pool.borrow_depth(), 0);
    }

    #[test]
    fn scratch_pool_nested_borrows() {
        let pool = ScratchPool::<f32>::new();
        pool.with_scratch(64, |s1| {
            s1[0] = 1.0;
            assert_eq!(pool.borrow_depth(), 1);
            pool.with_scratch(128, |s2| {
                s2[0] = 2.0;
                assert_eq!(pool.borrow_depth(), 2);
                assert_eq!(s1[0], 1.0);
                assert_eq!(s2[0], 2.0);
            });
            assert_eq!(pool.borrow_depth(), 1);
        });
        assert_eq!(pool.borrow_depth(), 0);
    }

    #[test]
    fn scratch_pool_overflow_to_owned() {
        let pool = ScratchPool::<f64>::new();
        fn nest(pool: &ScratchPool<f64>, depth: usize) {
            if depth == 0 { return; }
            pool.with_scratch(32, |_| { nest(pool, depth - 1); });
        }
        nest(&pool, MAX_POOL_SLOTS + 1);
        assert_eq!(pool.borrow_depth(), 0);
    }

    #[test]
    fn scratch_pool_exact_length() {
        let pool = ScratchPool::<f64>::new();
        // First call: grow to 256.
        pool.with_scratch(256, |s| assert_eq!(s.len(), 256));
        // Second call: request 128 — must get exactly 128, not 256.
        pool.with_scratch(128, |s| assert_eq!(s.len(), 128));
        // Third call: request 512 — grows.
        pool.with_scratch(512, |s| assert_eq!(s.len(), 512));
    }

    #[test]
    fn scratch_pool_no_rezero_on_reuse() {
        let pool = ScratchPool::<f64>::new();
        // Write data.
        pool.with_scratch(64, |s| {
            for (i, v) in s.iter_mut().enumerate() { *v = i as f64; }
        });
        // Reuse — data should still be present (not re-zeroed).
        pool.with_scratch(64, |s| {
            assert_eq!(s[0], 0.0); // first element was 0.0
            assert_eq!(s[63], 63.0); // last element was 63.0
        });
    }

    #[test]
    fn scratch_pool_returns_value() {
        let pool = ScratchPool::<f64>::new();
        let sum = pool.with_scratch(100, |scratch| {
            for (i, v) in scratch.iter_mut().enumerate() { *v = i as f64; }
            scratch.iter().sum::<f64>()
        });
        assert_eq!(sum, (0..100).map(|i| i as f64).sum::<f64>());
    }

    #[test]
    fn with_slot_capacity_preallocates() {
        let pool = ScratchPool::<f32>::with_slot_capacity(512);
        pool.with_scratch(256, |scratch| {
            assert_eq!(scratch.len(), 256);
            assert_eq!(scratch.as_ptr() as usize % DEFAULT_SCRATCH_ALIGN, 0);
        });
    }
}
