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

    /// Ensures the primary slot has at least `min_capacity` elements allocated,
    /// touching up only when the current capacity is smaller.
    ///
    /// Call this once (e.g. at thread startup or before the first hot loop)
    /// to amortise the allocation cost across the whole call-site lifetime.
    /// Calling `prewarm` while the pool is borrowed (`borrow_depth > 0`) is
    /// safe: the guard makes it impossible to have a live mutable reference to
    /// slot 0 when `borrow_depth == 0`, and `prewarm` only writes to slot 0
    /// when the pool is not borrowed.
    pub fn prewarm(&self, min_capacity: usize) {
        if self.borrow_depth.get() != 0 {
            // Pool in use — defer to next idle window; no panic.
            return;
        }
        // SAFETY: borrow_depth == 0 guarantees no live exclusive reference to
        // any slot exists. We briefly take an exclusive reference to slot 0
        // inside the check and grow, mirroring the pattern in `with_scratch`.
        let vec = unsafe { &mut *self.slots[0].get() };
        if vec.capacity() < min_capacity {
            vec.ensure_len(min_capacity);
            self.primary_capacity.set(vec.capacity());
        }
    }

    /// Like [`Self::with_scratch`] but provides an **uninitialized** slice.
    ///
    /// The caller receives a `*mut [T]` fat pointer instead of `&mut [T]`:
    /// the memory is valid and aligned but may contain arbitrary bit patterns.
    /// Initialisation (or a proof that all reads are preceded by writes) is the
    /// caller's responsibility.
    ///
    /// Use this when `T`'s zero-initialisation from [`AlignedVec::ensure_len`]
    /// dominates the profile and the caller can guarantee that every element is
    /// written before it is read. The pool itself always zeroes the buffer on
    /// the *first* use (via [`AlignedVec::with_capacity`]); "uninit" here means
    /// "no zeroing on growth" for elements that were already capacity but had
    /// been previously written.
    ///
    /// # Safety
    ///
    /// The caller **must** initialise every element in `[..n]` before reading
    /// any of them. Reading uninitialised memory is undefined behaviour.
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
            let _guard = BorrowGuard {
                depth: &self.borrow_depth,
                original: depth,
            };
            // SAFETY: exclusive access guaranteed by borrow_depth tracking.
            let vec = unsafe { &mut *self.slots[depth as usize].get() };
            // Grow if needed; ensure_len zero-fills only the newly added range.
            if n > vec.capacity() {
                vec.ensure_len(n);
                if depth == 0 {
                    self.primary_capacity.set(vec.capacity());
                }
            } else if n > vec.len() {
                // Within existing capacity but beyond current logical length:
                // extend without zeroing (the capacity was already committed).
                // SAFETY: `n <= vec.capacity()` (checked by the outer `else`
                // branch: capacity >= n because we only reach this arm when the
                // capacity check above passed without `ensure_len`).  The caller
                // of `with_scratch_uninit` is obligated to initialise every
                // element in `[..n]` before reading it; the function signature's
                // `# Safety` contract documents this obligation.
                unsafe { vec.set_len_unchecked(n) };
            }
            let ptr: *mut [T] = core::ptr::slice_from_raw_parts_mut(vec.as_mut_ptr(), n);
            f(ptr)
        } else {
            let mut owned = AlignedVec::with_capacity(n);
            // Capacity-only allocation; do not zero.
            // SAFETY: `with_capacity(n)` allocates exactly `n` elements worth of
            // backing storage, so `n <= owned.capacity()`.  The caller's
            // `# Safety` obligation covers initialisation of all `n` slots.
            unsafe { owned.set_len_unchecked(n) };
            let ptr: *mut [T] = core::ptr::slice_from_raw_parts_mut(owned.as_mut_ptr(), n);
            f(ptr)
        }
    }
}
