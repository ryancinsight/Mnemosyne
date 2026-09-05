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
    /// Per-depth high-water request, recorded by
    /// [`with_scratch_bounded`](Self::with_scratch_bounded) and honored by
    /// [`release`](Self::release). Provisioned slots keep capacity for their
    /// working set across a release; unprovisioned slots reclaim entirely.
    provisions: [Cell<usize>; MAX_POOL_SLOTS],
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
    slot_capacities: [Cell<usize>; MAX_POOL_SLOTS],
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
            provisions: [const { Cell::new(0) }; MAX_POOL_SLOTS],
            slot_capacities: [const { Cell::new(0) }; MAX_POOL_SLOTS],
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
            provisions: [const { Cell::new(0) }; MAX_POOL_SLOTS],
            // `mk()` gives every slot exactly `capacity` (a zero request yields
            // the zero-capacity dangling sentinel), so slot 0 starts here.
            slot_capacities: {
                let mirrors = [const { Cell::new(0) }; MAX_POOL_SLOTS];
                mirrors[0].set(capacity);
                mirrors
            },
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
                // Republish this slot's capacity to its mirror. Reading it
                // back through the live exclusive `vec` is the reborrow the
                // accessors themselves must not perform, so every slot keeps a
                // figure readable from outside the `UnsafeCell`.
                self.slot_capacities[depth as usize].set(vec.capacity());
            }
            debug_assert!(
                self.slot_capacities[depth as usize].get() == vec.capacity(),
                "slot capacity mirror drifted from the slot's actual capacity"
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

    /// Like [`with_scratch`](Self::with_scratch), but records the request for
    /// [`release`](Self::release).
    ///
    /// Each depth's largest-ever request becomes that slot's provision; a
    /// later [`release`] may reclaim everything a slot holds above it. The
    /// two forms share the slot storage, so a pool can be driven through
    /// either (or both) — only the provisions differ.
    ///
    /// # Panics
    ///
    /// Panics if `f` panics and leaves `self.borrow_depth` at `u8::MAX`, where
    /// the depth increment would wrap; [`with_scratch`] has the same bound via
    /// slot exhaustion, so this is not a new failure mode.
    ///
    /// [`release`]: Self::release
    /// [`with_scratch`]: Self::with_scratch
    #[inline]
    pub fn with_scratch_bounded<R>(&self, n: usize, f: impl FnOnce(&mut [T]) -> R) -> R {
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
            let idx = depth as usize;
            // Record this depth's high-water request before the buffer is
            // grown, so `release` can distinguish the requested size from
            // growth-policy headroom.
            let provision = &self.provisions[idx];
            provision.set(provision.get().max(n));
            // SAFETY: exclusive access guaranteed by borrow_depth tracking.
            // Each nesting level gets its own slot index.
            let vec = unsafe { &mut *self.slots[idx].get() };
            if n > vec.len() {
                vec.ensure_len(n);
                self.slot_capacities[idx].set(vec.capacity());
            }
            debug_assert!(
                self.slot_capacities[idx].get() == vec.capacity(),
                "slot capacity mirror drifted from the slot's actual capacity"
            );
            debug_assert_eq!(
                vec.as_mut_ptr() as usize % T::ALIGN_BYTES,
                0,
                "Scratch buffer not aligned to {} bytes",
                T::ALIGN_BYTES
            );
            let slice = &mut vec.as_mut_slice()[..n];
            f(slice)
        } else {
            let mut owned = AlignedVec::with_capacity(n);
            owned.ensure_len(n);
            f(owned.as_mut_slice())
        }
    }

    /// Reclaims every slot's storage above its recorded provision.
    ///
    /// With [`with_scratch_bounded`](Self::with_scratch_bounded) as the only
    /// entry point, a provision is the largest request ever seen at that
    /// depth; the slot keeps capacity for it (warm reuse stays allocation-free)
    /// and surrenders everything above — growth headroom included, so the
    /// retained steady state is exactly the working set. Slots whose provision
    /// is zero are dropped entirely. A slot is reclaimed only when its depth
    /// is idle; busy slots are skipped, never torn from under a live borrow.
    ///
    /// The intended quiescent rhythm: run transforms normally, then call this
    /// when the workload idles — not on every `with_scratch` exit, which would
    /// reintroduce the churn the pool exists to remove. Provisions persist, so
    /// repeated release/idle cycles converge; a smaller *steady-state* working
    /// set needs [`reset`](Self::reset).
    ///
    /// Returns the per-slot capacities after reclamation. A slot that is
    /// currently borrowed reports its provision instead: its live capacity is
    /// not observable without deriving a reference under an existing exclusive
    /// borrow, the same aliasing [`capacity`](Self::capacity) exists to avoid.
    pub fn release(&self) -> [usize; MAX_POOL_SLOTS] {
        let mut capacities = [0usize; MAX_POOL_SLOTS];
        for (idx, slot) in self.slots.iter().enumerate() {
            let provision = self.provisions[idx].get();
            if self.borrow_depth.get() > idx as u8 {
                capacities[idx] = provision;
                continue;
            }
            // SAFETY: exclusive access — the depth guard above proved this
            // slot's nesting level is not on the stack, so no borrow of the
            // slot can be live.
            let vec = unsafe { &mut *slot.get() };
            if provision == 0 {
                if vec.capacity() != 0 {
                    // Drop returns the allocation, then the sentinel lands.
                    *vec = AlignedVec::dangling();
                    self.slot_capacities[idx].set(0);
                }
            } else if vec.capacity() > provision {
                vec.shrink_to(provision);
                // No `clear`: reuse is explicitly not re-zeroed (see
                // `with_scratch`), and zeroing here would put an O(n)
                // memset on the warm path — the exact churn the pool
                // exists to remove. The next growth re-zeros its new
                // range as always.
                self.slot_capacities[idx].set(vec.capacity());
            }
            capacities[idx] = vec.capacity();
        }
        capacities
    }

    /// Clears the recorded provisions so a later [`release`](Self::release)
    /// reclaims every slot entirely.
    ///
    /// For a full working-set changeover (a consumer tearing down one workload
    /// and starting another): reset, then run the new workload through
    /// [`with_scratch_bounded`](Self::with_scratch_bounded), then release.
    /// Slots that are not idle keep their buffers; the next release sees their
    /// cleared provisions and reclaims them.
    pub fn reset(&self) {
        for provision in &self.provisions {
            provision.set(0);
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
    ///
    /// Every slot carries such a mirror; see [`Self::total_capacity_bytes`] for
    /// the sum across all of them.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.slot_capacities[0].get()
    }

    /// Ensures the primary slot has capacity for at least `min_capacity`
    /// elements, growing it if necessary. No-op when the pool is borrowed.
    #[inline]
    pub fn prewarm(&self, min_capacity: usize) {
        if self.borrow_depth.get() != 0 {
            return;
        }
        // SAFETY: borrow_depth == 0 so no live exclusive reference to slot 0.
        let vec = unsafe { &mut *self.slots[0].get() };
        if vec.capacity() < min_capacity {
            vec.ensure_len(min_capacity);
            self.slot_capacities[0].set(vec.capacity());
        }
    }

    /// Sum of backing capacities across all slots, in bytes.
    #[inline]
    pub fn total_capacity_bytes(&self) -> usize {
        // Read from the per-slot mirrors, never through the slots
        // themselves: a live `with_scratch` borrow holds one slot exclusively,
        // and the mirrors are maintained outside the `UnsafeCell`s precisely so
        // this stays a total — a borrow-time branch returning slot 0 alone
        // would contradict the documented sum.
        self.slot_capacities
            .iter()
            .map(|mirror| mirror.get().saturating_mul(core::mem::size_of::<T>()))
            .fold(0usize, usize::saturating_add)
    }

    /// Releases all slot allocations when not borrowed. No-op when borrowed.
    #[inline]
    pub fn shrink_all_slots(&self) {
        if self.borrow_depth.get() != 0 {
            return;
        }
        for (i, slot) in self.slots.iter().enumerate() {
            // SAFETY: borrow_depth == 0 — no live exclusive references.
            let vec = unsafe { &mut *slot.get() };
            *vec = AlignedVec::dangling();
            self.slot_capacities[i].set(0);
        }
    }

    /// Returns `true` when the pool has at least one slot available for a
    /// new borrow (`borrow_depth < MAX_POOL_SLOTS`).
    #[inline]
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.borrow_depth.get() < MAX_POOL_SLOTS as u8
    }

    /// Returns the backing capacity of slot `idx`, or `0` when `idx` is out
    /// of range. Callable at any time including during a live borrow.
    #[inline]
    #[must_use]
    pub fn slot_capacity(&self, idx: usize) -> usize {
        self.slot_capacities.get(idx).map_or(0, |c| c.get())
    }

    /// Like [`with_scratch`][Self::with_scratch] but provides uninitialized
    /// memory via a raw pointer. The caller must initialize all elements.
    ///
    /// # Safety
    ///
    /// Every element of the returned slice must be initialized before any
    /// safe read on the same allocation.
    pub unsafe fn with_scratch_uninit<R>(&self, n: usize, f: impl FnOnce(*mut [T]) -> R) -> R {
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
            // SAFETY: borrow_depth tracking ensures exclusive access to this slot.
            let vec = unsafe { &mut *self.slots[depth as usize].get() };
            if vec.capacity() < n {
                vec.ensure_len(n);
                self.slot_capacities[depth as usize].set(vec.capacity());
            }
            let raw = core::ptr::slice_from_raw_parts_mut(vec.as_mut_ptr(), n);
            // The length is published only after `f` returns normally. When the
            // slot already has spare capacity no `ensure_len` runs, so
            // `[len, n)` is uninitialized while `f` executes; publishing `n`
            // first would leave that length behind on an unwind, and the next
            // safe `with_scratch(n, ..)` — seeing `n <= len` — would skip
            // `ensure_len` and hand out a slice over uninitialized elements.
            let result = f(raw);
            // SAFETY: `f` returned normally, discharging the caller's contract
            // to initialize `[0, n)`; capacity >= n was established above.
            unsafe { vec.set_len_unchecked(n) };
            result
        } else {
            let mut owned = AlignedVec::with_capacity(n);
            // SAFETY: caller initializes before safe reads.
            unsafe { owned.set_len_unchecked(n) };
            let raw = core::ptr::slice_from_raw_parts_mut(owned.as_mut_ptr(), n);
            f(raw)
        }
    }
}
