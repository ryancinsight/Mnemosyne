//! Scratch buffer pool: a depth-tracked set of reusable aligned scratch
//! buffers for temporal allocations.
//!
//! The pool is decomposed by responsibility (each submodule is private; its
//! methods are re-exported through the `impl` blocks below):
//! - `borrow` — the PROVISION-generic `borrow_slot`, `with_scratch`,
//!   `with_scratch_bounded`, and `with_scratch_uninit` borrow entry points.
//! - `manage` — provision-aware `release`, `reset`, `prewarm`, `preload`,
//!   and `shrink_all_slots` lifecycle methods.
//! - `query` — read-only `borrow_depth`, `capacity`, `slot_capacity`,
//!   `is_available`, `total_capacity_bytes` accessors.
//!
//! `mod.rs` keeps only the struct shape, `MAX_POOL_SLOTS`, and the
//! `new`/`with_slot_capacity`/`Default` constructors.

mod borrow;
mod manage;
mod query;

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
    pub(super) slots: [UnsafeCell<AlignedVec<T>>; MAX_POOL_SLOTS],
    pub(super) borrow_depth: Cell<u8>,
    /// Per-depth high-water request, recorded by
    /// [`with_scratch_bounded`](Self::with_scratch_bounded) and honored by
    /// [`release`](Self::release). Provisioned slots keep capacity for their
    /// working set across a release; unprovisioned slots reclaim entirely.
    pub(super) provisions: [Cell<usize>; MAX_POOL_SLOTS],
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
    pub(super) slot_capacities: [Cell<usize>; MAX_POOL_SLOTS],
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
            // the zero-capacity dangling sentinel), so every slot's mirror must
            // begin with the same warm capacity. This keeps the public
            // `slot_capacity`/`total_capacity_bytes` figures in sync with the
            // actual backing allocations across all prewarmed slots.
            slot_capacities: {
                let mut mirrors = [const { Cell::new(0) }; MAX_POOL_SLOTS];
                for cell in &mut mirrors {
                    cell.set(capacity);
                }
                mirrors
            },
        }
    }
}
