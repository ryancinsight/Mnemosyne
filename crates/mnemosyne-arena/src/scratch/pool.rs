//! Scratch buffer pool implementation for temporal allocations.

use super::aligned_vec::AlignedVec;
use super::element::ScratchElement;
use core::cell::{Cell, UnsafeCell};

/// Maximum concurrent borrows (recursive/nested calls) the pool supports.
pub const MAX_POOL_SLOTS: usize = 4;

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
    borrow_depth: Cell<u8>,
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
            borrow_depth: Cell::new(0),
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
            borrow_depth: Cell::new(0),
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
