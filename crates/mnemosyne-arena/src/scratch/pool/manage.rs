//! Lifecycle management for [`super::ScratchPool`]: provision-aware
//! `release`, provision reset, warm-up helpers, and full-reclaim.

use super::super::aligned_vec::AlignedVec;
use super::super::element::ScratchElement;
use super::{MAX_POOL_SLOTS, ScratchPool};

impl<T: ScratchElement> ScratchPool<T> {
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

    /// Prewarms multiple slots in one call.
    ///
    /// `sizes[i]` specifies the minimum capacity for slot `i` (depth `i`).
    /// Out-of-range indices or a borrowed slot are silently skipped.
    /// Entries of `0` skip that slot.
    #[inline]
    pub fn preload(&self, sizes: &[usize]) {
        if self.borrow_depth.get() != 0 {
            return;
        }
        for (idx, &min_cap) in sizes.iter().enumerate().take(MAX_POOL_SLOTS) {
            if min_cap == 0 {
                continue;
            }
            // SAFETY: borrow_depth == 0; idx < MAX_POOL_SLOTS.
            let vec = unsafe { &mut *self.slots[idx].get() };
            if vec.capacity() < min_cap {
                vec.ensure_len(min_cap);
                self.slot_capacities[idx].set(vec.capacity());
            }
        }
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
}
