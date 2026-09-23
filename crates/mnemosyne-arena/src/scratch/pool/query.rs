//! Read-only queries for [`super::ScratchPool`]: depth, capacity mirrors,
//! slot availability, and total byte footprint.
//!
//! All methods here read from the `Cell` mirrors maintained outside the
//! `UnsafeCell` slots, so they are safe to call even from inside a live
//! `with_scratch` borrow of the same pool.

use super::super::element::ScratchElement;
use super::ScratchPool;

impl<T: ScratchElement> ScratchPool<T> {
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

    /// Returns `true` when the pool has at least one slot available for a
    /// new borrow (`borrow_depth < MAX_POOL_SLOTS`).
    #[inline]
    #[must_use]
    pub fn is_available(&self) -> bool {
        use super::MAX_POOL_SLOTS;
        self.borrow_depth.get() < MAX_POOL_SLOTS as u8
    }

    /// Returns the backing capacity of slot `idx`, or `0` when `idx` is out
    /// of range. Callable at any time including during a live borrow.
    #[inline]
    #[must_use]
    pub fn slot_capacity(&self, idx: usize) -> usize {
        self.slot_capacities.get(idx).map_or(0, |c| c.get())
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
}
