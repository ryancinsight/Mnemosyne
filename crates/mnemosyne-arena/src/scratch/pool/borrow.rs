//! Borrow entry points for [`super::ScratchPool`]: the depth-tracked
//! `borrow_slot` dispatch, the safe `with_scratch`/`with_scratch_bounded`
//! wrappers, and the unsafe `with_scratch_uninit` raw-pointer variant.
//!
//! All three forms share the same RAII `BorrowGuard` to restore `borrow_depth`
//! on unwind, and diverge only in the PROVISION const-param that controls
//! whether the high-water mark is updated.

use super::super::aligned_vec::AlignedVec;
use super::super::element::ScratchElement;
use super::{MAX_POOL_SLOTS, ScratchPool};
use core::cell::Cell;

impl<T: ScratchElement> ScratchPool<T> {
    /// Provides a mutable aligned scratch slice of **exactly** `n` elements
    /// to the closure. Borrow depth is released when the closure returns.
    ///
    /// If a pool slot is available, the closure receives a direct `&mut [T]`
    /// into the pooled buffer (zero-copy). If all slots are exhausted (nested
    /// recursive calls), a temporary buffer is allocated instead.
    #[inline]
    pub fn with_scratch<R>(&self, n: usize, f: impl FnOnce(&mut [T]) -> R) -> R {
        self.borrow_slot::<false, R>(n, f)
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
        self.borrow_slot::<true, R>(n, f)
    }

    /// Shared implementation for [`with_scratch`] and [`with_scratch_bounded`].
    ///
    /// `PROVISION` is a compile-time constant: when `false` the provision
    /// tracking branch is eliminated by the optimizer and the two public forms
    /// have identical hot-path machine code, differing only in the
    /// cold-provision-update path.
    #[inline]
    pub(super) fn borrow_slot<const PROVISION: bool, R>(
        &self,
        n: usize,
        f: impl FnOnce(&mut [T]) -> R,
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
            let idx = depth as usize;
            if PROVISION {
                // Record this depth's high-water request so `release` can
                // distinguish the requested size from growth-policy headroom.
                let provision = &self.provisions[idx];
                provision.set(provision.get().max(n));
            }
            // SAFETY: exclusive access guaranteed by borrow_depth tracking.
            // Each nesting level gets its own slot index.
            let vec = unsafe { &mut *self.slots[idx].get() };
            if n > vec.len() {
                // The bounded path uses exact growth so the slot's capacity
                // tracks the provision precisely: `grow_geometric` would
                // overshoot (e.g. a prior capacity of 8704 doubles to 17 408
                // for a 16 384 request), making a subsequent `release`
                // `shrink_to(provision)` a real reallocation instead of a
                // no-op. The unbounded path keeps geometric growth for
                // amortized reuse.
                if PROVISION {
                    vec.ensure_len_exact(n);
                } else {
                    vec.ensure_len(n);
                }
                // Republish this slot's capacity to its mirror. Reading it
                // back through the live exclusive `vec` is the reborrow the
                // accessors themselves must not perform, so every slot keeps a
                // figure readable from outside the `UnsafeCell`.
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
