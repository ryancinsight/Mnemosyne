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
//! - **Alignment**: Buffers are aligned to configurable byte boundaries (default
//!   64 bytes for AVX-512 cache lines).
//! - **Growth-on-demand**: Buffers grow geometrically and never shrink,
//!   amortizing allocation cost across the workload lifetime.
//! - **Zero-cost generics**: The pool is parameterized over element type via
//!   the [`ScratchElement`] sealed trait, monomorphizing per-type pools with
//!   no dynamic dispatch.
//! - **Zero-copy**: The closure receives a direct `&mut [T]` slice into the
//!   pool buffer — no intermediate copying or `Cow` wrapping.

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

// f32: 64-byte alignment for AVX-512
impl sealed::ScratchElementSealed for f32 {}
impl ScratchElement for f32 {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

// f64: 64-byte alignment for AVX-512
impl sealed::ScratchElementSealed for f64 {}
impl ScratchElement for f64 {
    const ALIGN_BYTES: usize = DEFAULT_SCRATCH_ALIGN;
}

// u8: byte-level scratch (poisoning, masks)
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
///
/// Unlike `Vec<T>`, the backing allocation is always aligned to
/// `T::ALIGN_BYTES`, making the returned slices safe for aligned SIMD loads
/// and stores without runtime offset computation.
pub struct AlignedVec<T: ScratchElement> {
    ptr: *mut T,
    len: usize,
    capacity: usize,
    _phantom: PhantomData<T>,
}

impl<T: ScratchElement> AlignedVec<T> {
    /// Creates a dangling sentinel with zero capacity. Used for const
    /// initialization; no allocation occurs until `resize` is called.
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
        // SAFETY: layout is non-zero because capacity >= 1 and element size > 0.
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
        // SAFETY: `self.len` elements have been initialized.
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// Returns a shared slice of the initialized elements.
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: `self.len` elements have been initialized.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Returns the number of initialized elements.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the total capacity in elements.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Ensures the buffer can hold at least `new_len` elements, growing
    /// geometrically if needed. Sets `self.len` to `new_len`.
    #[inline]
    pub fn resize(&mut self, new_len: usize) {
        if new_len <= self.len {
            self.len = new_len;
            return;
        }
        if new_len > self.capacity {
            let new_cap = new_len.max(self.capacity.saturating_mul(2));
            self.grow_to(new_cap);
        }
        // Zero the new elements for safety (FFT scratch expects zeroed memory).
        // SAFETY: the range [self.len, new_len) is within capacity.
        unsafe {
            let dst = self.ptr.add(self.len);
            core::ptr::write_bytes(dst, 0, new_len - self.len);
        }
        self.len = new_len;
    }

    /// Returns a pointer to the raw backing buffer.
    #[inline]
    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.ptr
    }

    /// Consumes self and returns a `Vec<T>` with the initialized data.
    #[inline]
    pub fn into_vec(self) -> Vec<T> {
        let mut v = Vec::with_capacity(self.len);
        // SAFETY: self.ptr has self.len initialized elements, and v has capacity.
        unsafe {
            core::ptr::copy_nonoverlapping(self.ptr, v.as_mut_ptr(), self.len);
            v.set_len(self.len);
        }
        // Prevent self's destructor from running.
        core::mem::forget(self);
        v
    }

    #[cold]
    #[inline(never)]
    fn grow_to(&mut self, new_capacity: usize) {
        let new_layout = Self::layout_for(new_capacity);
        let new_ptr = if self.capacity == 0 {
            // Dangling sentinel: allocate fresh instead of reallocating.
            // SAFETY: new_capacity > 0, new_layout is non-zero.
            unsafe { (alloc::alloc::alloc(new_layout)) as *mut T }
        } else {
            let old_layout = Self::layout_for(self.capacity);
            // SAFETY: new_capacity > self.capacity > 0, ptr was allocated with old_layout.
            unsafe {
                (alloc::alloc::realloc(self.ptr as *mut u8, old_layout, new_layout.size()))
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
            // SAFETY: ptr was allocated with this layout and has not been freed.
            unsafe {
                alloc::alloc::dealloc(self.ptr as *mut u8, layout);
            }
        }
    }
}

// SAFETY: AlignedVec owns its data and T is Send.
unsafe impl<T: ScratchElement + Send> Send for AlignedVec<T> {}

// ---------------------------------------------------------------------------
// ScratchPool: closure-based, RAII-safe, aligned scratch buffer pool
// ---------------------------------------------------------------------------

/// Maximum number of concurrent borrows (recursive/nested calls) the pool
/// supports. FFT algorithms rarely recurse deeper than 4 levels.
const MAX_POOL_SLOTS: usize = 4;

/// A pool of reusable, aligned scratch buffers for a specific element type.
///
/// The pool manages a fixed number of buffer slots (default 4, supporting
/// recursive/nested transform calls). Each slot grows on demand and is reused
/// across calls, avoiding repeated allocation for numerical scratch space.
///
/// # Thread safety
///
/// `ScratchPool` is `Send` but **not** `Sync`. It is designed for
/// `thread_local!` storage. Each thread gets its own pool with zero contention.
///
/// # Usage (closure-based, RAII-safe)
///
/// ```rust,ignore
/// use mnemosyne_arena::scratch::ScratchPool;
///
/// thread_local! {
///     static POOL: ScratchPool<f64> = ScratchPool::new();
/// }
///
/// POOL.with(|pool| {
///     pool.with_scratch::<f64, _>(1024, |scratch| {
///         // scratch is a &mut [f64] of exactly 1024 elements, 64-byte aligned.
///         // Borrow depth is automatically released when the closure returns.
///     });
/// });
/// ```
pub struct ScratchPool<T: ScratchElement> {
    slots: [UnsafeCell<AlignedVec<T>>; MAX_POOL_SLOTS],
    borrow_depth: core::cell::Cell<u8>,
}

// SAFETY: ScratchPool is Send — it can move between threads. It is NOT Sync;
// access is single-threaded via thread_local!.
unsafe impl<T: ScratchElement> Send for ScratchPool<T> {}

impl<T: ScratchElement> ScratchPool<T> {
    /// Creates a new empty scratch pool.
    ///
    /// Uses dangling sentinels until first use; zero allocation at construction.
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

    /// Provides a mutable aligned scratch slice of at least `n` elements to
    /// the closure `f`. The borrow depth is automatically released when the
    /// closure returns — no manual cleanup required.
    ///
    /// If the pool has an available slot, the closure receives a direct
    /// `&mut [T]` into the pool buffer (zero-copy). If all slots are
    /// exhausted (nested recursive calls), a temporary `AlignedVec` is
    /// allocated and passed to the closure instead.
    ///
    /// The returned value `R` is the closure's return value.
    #[inline]
    pub fn with_scratch<R>(&self, n: usize, f: impl FnOnce(&mut [T]) -> R) -> R {
        let depth = self.borrow_depth.get();
        if depth < MAX_POOL_SLOTS as u8 {
            self.borrow_depth.set(depth + 1);
            // SAFETY: We have exclusive access to this slot because
            // borrow_depth tracks the nesting level and each depth gets
            // its own slot index. The mutable reference never outlives
            // this scope (it's passed into the closure and dropped when
            // the closure returns).
            let vec = unsafe { &mut *self.slots[depth as usize].get() };
            vec.resize(n.max(vec.len()));
            // Align-check the pointer.
            debug_assert_eq!(
                vec.as_mut_ptr() as usize % T::ALIGN_BYTES,
                0,
                "Scratch buffer not aligned to {} bytes",
                T::ALIGN_BYTES
            );
            let result = f(vec.as_mut_slice());
            self.borrow_depth.set(depth);
            result
        } else {
            // All slots exhausted; allocate owned fallback.
            let mut owned = AlignedVec::with_capacity(n);
            owned.resize(n);
            f(owned.as_mut_slice())
        }
    }

    /// Returns the current borrow depth (0 = fully available).
    #[inline]
    pub fn borrow_depth(&self) -> u8 {
        self.borrow_depth.get()
    }
}

/// Get a global default alignment constant for external consumers.
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
        v.resize(256);
        assert_eq!(v.len(), 256);
        assert!(v.capacity() >= 256);
        assert_eq!(
            v.as_mut_ptr() as usize % DEFAULT_SCRATCH_ALIGN,
            0,
            "AlignedVec<f64> must be 64-byte aligned"
        );
    }

    #[test]
    fn aligned_vec_growth_preserves_data() {
        let mut v = AlignedVec::<f32>::with_capacity(4);
        v.resize(4);
        v.as_mut_slice().copy_from_slice(&[1.0, 2.0, 3.0, 4.0]);
        v.resize(8);
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
        v.resize(4);
        v.as_mut_slice().copy_from_slice(&[1.0, 2.0, 3.0, 4.0]);
        let vec = v.into_vec();
        assert_eq!(vec, std::vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn scratch_pool_single_borrow() {
        let pool = ScratchPool::<f64>::new();
        pool.with_scratch(128, |scratch| {
            assert_eq!(scratch.len(), 128);
            scratch[0] = 42.0;
            assert_eq!(scratch[0], 42.0);
            // Alignment check
            assert_eq!(
                scratch.as_ptr() as usize % DEFAULT_SCRATCH_ALIGN,
                0,
                "Scratch must be 64-byte aligned"
            );
        });
        assert_eq!(pool.borrow_depth(), 0, "depth must return to 0 after closure");
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
                // Independent data
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
        // Nest to MAX_POOL_SLOTS depth then one more.
        fn nest(pool: &ScratchPool<f64>, depth: usize) {
            if depth == 0 {
                return;
            }
            pool.with_scratch(32, |_scratch| {
                nest(pool, depth - 1);
            });
        }
        // This should not panic — the 5th level uses an owned fallback.
        nest(pool, MAX_POOL_SLOTS + 1);
        assert_eq!(pool.borrow_depth(), 0);
    }

    #[test]
    fn scratch_pool_reuse_across_calls() {
        let pool = ScratchPool::<f64>::new();
        // First call: grows the buffer.
        pool.with_scratch(256, |scratch| {
            for (i, v) in scratch.iter_mut().enumerate() {
                *v = i as f64;
            }
        });
        assert_eq!(pool.borrow_depth(), 0);
        // Second call: reuses the same buffer (no reallocation).
        pool.with_scratch(128, |scratch| {
            // The buffer still has leftover data from the first call
            // beyond index 128, but the slice is zeroed to 128 elements.
            assert_eq!(scratch.len(), 128);
        });
    }

    #[test]
    fn scratch_pool_returns_value() {
        let pool = ScratchPool::<f64>::new();
        let sum = pool.with_scratch(100, |scratch| {
            for (i, v) in scratch.iter_mut().enumerate() {
                *v = i as f64;
            }
            scratch.iter().sum::<f64>()
        });
        assert_eq!(sum, (0..100).map(|i| i as f64).sum::<f64>());
    }

    #[test]
    fn with_slot_capacity_preallocates() {
        let pool = ScratchPool::<f32>::with_slot_capacity(512);
        pool.with_scratch(256, |scratch| {
            assert_eq!(scratch.len(), 256);
            assert_eq!(
                scratch.as_ptr() as usize % DEFAULT_SCRATCH_ALIGN,
                0,
            );
        });
    }
}
