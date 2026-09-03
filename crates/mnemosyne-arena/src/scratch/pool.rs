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
    /// Slot 0's capacity, republished by the borrow that grows it.
    ///
    /// [`ScratchPool::capacity`] is reachable from inside a live
    /// [`ScratchPool::with_scratch`] borrow through entirely safe code (both
    /// take `&self`, and the pool's documented home is a `thread_local!`), so
    /// the accessor must not derive a reference into a slot that borrow already
    /// holds exclusively. Mirroring the figure outside the `UnsafeCell` removes
    /// the aliasing by construction rather than forbidding the call.
    ///
    /// Slot 0's capacity changes only where this is written: construction, and
    /// the grow branch of a depth-0 `with_scratch` (slot index equals borrow
    /// depth, so only depth 0 touches slot 0). A `debug_assert!` in
    /// `with_scratch` fails the tests if a future mutation path escapes that
    /// set.
    primary_capacity: Cell<usize>,
}

// SAFETY: a `ScratchPool` uniquely owns its slot buffers (each `AlignedVec` owns
// its heap storage with no aliasing), so moving the whole pool to another thread
// is sound. It is deliberately *not* `Sync`: the `UnsafeCell` slots and the
// `Cell` borrow-depth and capacity fields are guarded only by single-threaded
// `borrow_depth` tracking, which assumes one thread at a time (`thread_local!`
// storage), so it must never be shared by reference across threads.
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
            primary_capacity: Cell::new(0),
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
            // `mk()` gives every slot exactly `capacity` (a zero request yields
            // the zero-capacity dangling sentinel), so slot 0 starts here.
            primary_capacity: Cell::new(capacity),
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
        struct BorrowGuard<'a> {
            depth: &'a Cell<u8>,
            original: u8,
        }

        impl Drop for BorrowGuard<'_> {
            #[inline(always)]
            fn drop(&mut self) {
                self.depth.set(self.original);
            }
        }

        let depth = self.borrow_depth.get();
        if depth < MAX_POOL_SLOTS as u8 {
            self.borrow_depth.set(depth + 1);
            let _guard = BorrowGuard {
                depth: &self.borrow_depth,
                original: depth,
            };
            // SAFETY: exclusive access guaranteed by borrow_depth tracking.
            // Each nesting level gets its own slot index.
            let vec = unsafe { &mut *self.slots[depth as usize].get() };
            // Ensure the buffer is large enough. If the buffer was already
            // grown by a prior call, reuse it without re-zeroing (only newly
            // added elements are zeroed by ensure_len).
            if n > vec.len() {
                vec.ensure_len(n);
                if depth == 0 {
                    // Slot index equals borrow depth, so this is the only place
                    // slot 0's capacity can change after construction. Reading
                    // it through the live exclusive `vec` is the reborrow the
                    // accessor itself must not perform.
                    self.primary_capacity.set(vec.capacity());
                }
            }
            debug_assert!(
                depth != 0 || self.primary_capacity.get() == vec.capacity(),
                "primary_capacity drifted from slot 0's actual capacity"
            );
            debug_assert_eq!(
                vec.as_mut_ptr() as usize % T::ALIGN_BYTES,
                0,
                "Scratch buffer not aligned to {} bytes",
                T::ALIGN_BYTES
            );
            // Return exactly `n` elements (not the full buffer).
            let slice = &mut vec.as_mut_slice()[..n];
            f(slice)
        } else {
            // All slots exhausted; allocate owned fallback.
            let mut owned = AlignedVec::with_capacity(n);
            owned.ensure_len(n);
            f(owned.as_mut_slice())
        }
    }

    /// Returns the current borrow depth (0 = fully available).
    #[inline]
    pub fn borrow_depth(&self) -> u8 {
        self.borrow_depth.get()
    }

    /// Returns the capacity of the first slot (primary buffer).
    ///
    /// Callable at any time, including from inside a live
    /// [`Self::with_scratch`] borrow of that same slot. The figure is read from
    /// a mirror maintained outside the slot's `UnsafeCell`, so the accessor
    /// never derives a reference that could alias the exclusive one the borrow
    /// holds — the reentrant call is sound rather than merely undetected, and it
    /// neither panics nor reports a stale value.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.primary_capacity.get()
    }

    /// Ensures the primary slot has capacity for at least `min_capacity` elements,
    /// growing it if necessary. A no-op when the pool is currently borrowed.
    ///
    /// Call once at thread startup to amortise the first-use allocation cost:
    /// subsequent [`Self::with_scratch`] calls up to `min_capacity` will reuse
    /// the pre-grown buffer without reallocating.
    #[inline]
    pub fn prewarm(&self, min_capacity: usize) {
        // Cannot safely touch slot 0 while it is borrowed.
        if self.borrow_depth.get() != 0 {
            return;
        }
        // SAFETY: borrow_depth == 0, so no live exclusive reference to slot 0
        // exists. We take a brief exclusive borrow to grow it.
        let vec = unsafe { &mut *self.slots[0].get() };
        if vec.capacity() < min_capacity {
            vec.ensure_len(min_capacity);
            self.primary_capacity.set(vec.capacity());
        }
    }

    /// Like [`with_scratch`][Self::with_scratch] but provides an uninitialised
    /// slice via a raw pointer.
    ///
    /// The caller receives a `*mut [T]` fat pointer into the pool buffer.
    /// Alignment is guaranteed; contents are undefined. The caller is
    /// responsible for initialising every element before reading any of them.
    ///
    /// Use this when the all-zero initialisation performed by
    /// [`ensure_len`][crate::scratch::AlignedVec::ensure_len] on growth would
    /// be wasted because the caller overwrites the entire slice anyway.
    ///
    /// # Safety
    ///
    /// Every element of the returned slice **must** be initialised before any
    /// read through a safe interface (`as_slice`, `Deref`, etc.) on the same
    /// `AlignedVec`. Violating this condition is undefined behaviour.
    pub unsafe fn with_scratch_uninit<R>(
        &self,
        n: usize,
        f: impl FnOnce(*mut [T]) -> R,
    ) -> R {
        struct BorrowGuard<'a> {
            depth: &'a Cell<u8>,
            original: u8,
        }
        impl Drop for BorrowGuard<'_> {
            #[inline(always)]
            fn drop(&mut self) {
                self.depth.set(self.original);
            }
        }

        let depth = self.borrow_depth.get();
        if depth < MAX_POOL_SLOTS as u8 {
            self.borrow_depth.set(depth + 1);
            let _guard = BorrowGuard { depth: &self.borrow_depth, original: depth };
            // SAFETY: borrow_depth tracking ensures exclusive access to this slot.
            let vec = unsafe { &mut *self.slots[depth as usize].get() };
            // Grow only if capacity is insufficient; skip zeroing by using the raw
            // pointer interface.
            if vec.capacity() < n {
                // Use ensure_len to grow (which zeroes only the new range), then
                // reset len so the caller treats the full slice as uninitialized.
                vec.ensure_len(n);
                if depth == 0 {
                    self.primary_capacity.set(vec.capacity());
                }
            }
            // SAFETY: `set_len_unchecked` makes the slot appear `n` elements long
            // for the duration of the closure. The caller's safety contract requires
            // them to initialise every element before returning.
            unsafe { vec.set_len_unchecked(n) };
            let raw: *mut [T] = core::ptr::slice_from_raw_parts_mut(vec.as_mut_ptr(), n);
            f(raw)
        } else {
            // Fallback: heap-allocate a temporary uninit buffer.
            let mut owned = AlignedVec::with_capacity(n);
            // SAFETY: capacity == n; caller will initialise every element.
            unsafe { owned.set_len_unchecked(n) };
            let raw: *mut [T] = core::ptr::slice_from_raw_parts_mut(owned.as_mut_ptr(), n);
            f(raw)
        }
    }
}
