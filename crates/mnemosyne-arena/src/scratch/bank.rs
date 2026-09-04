//! Scratch bank implementation for keeping multiple related pools together.

use super::element::ScratchElement;
use super::pool::{MAX_POOL_SLOTS, ScratchPool};

/// A fixed set of same-typed scratch pools for domain-specific temporary roles.
///
/// Transform crates commonly need several independent thread-local scratch
/// buffers for one element type: e.g. Stockham data, PFA data, Rader padding,
/// and Bluestein chirps. `ScratchBank<T, N>` keeps those roles in one
/// const-generic provider-owned container while preserving the same zero-copy
/// [`ScratchPool::with_scratch`] access contract for each slot.
pub struct ScratchBank<T: ScratchElement, const N: usize> {
    pools: [ScratchPool<T>; N],
}

// SAFETY: a `ScratchBank` owns its `N` `ScratchPool`s by value with no shared
// aliasing, and each `ScratchPool<T>` is itself `Send` (its buffers uniquely own
// their storage). Transferring the whole bank to another thread is therefore
// sound. Like `ScratchPool` it is deliberately not `Sync` (single-thread,
// `thread_local!`-owned access only).
unsafe impl<T: ScratchElement, const N: usize> Send for ScratchBank<T, N> {}

impl<T: ScratchElement, const N: usize> Default for ScratchBank<T, N> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ScratchElement, const N: usize> ScratchBank<T, N> {
    /// Creates a bank of empty scratch pools.
    #[inline]
    pub const fn new() -> Self {
        Self {
            pools: [const { ScratchPool::new() }; N],
        }
    }

    /// Runs `f` with scratch from slot `INDEX`, sized to exactly `n` elements.
    ///
    /// `INDEX` is a const generic so role selection is resolved at compile
    /// time at monomorphized call sites.
    ///
    /// # Panics
    ///
    /// Panics when `INDEX >= N`.
    #[inline]
    pub fn with_scratch<const INDEX: usize, R>(
        &self,
        n: usize,
        f: impl FnOnce(&mut [T]) -> R,
    ) -> R {
        assert!(INDEX < N, "ScratchBank slot index out of range");
        self.pools[INDEX].with_scratch(n, f)
    }

    /// Like [`with_scratch`](Self::with_scratch), but records the request so a
    /// later [`release`](Self::release) can reclaim above the working set.
    /// See [`ScratchPool::with_scratch_bounded`] for the full contract.
    ///
    /// # Panics
    ///
    /// Panics when `INDEX >= N`.
    #[inline]
    pub fn with_scratch_bounded<const INDEX: usize, R>(
        &self,
        n: usize,
        f: impl FnOnce(&mut [T]) -> R,
    ) -> R {
        assert!(INDEX < N, "ScratchBank slot index out of range");
        self.pools[INDEX].with_scratch_bounded(n, f)
    }

    /// Reclaims every pool's storage above its recorded provision.
    /// See [`ScratchPool::release`] for the contract and the quiescent-calling
    /// rhythm this is designed for.
    ///
    /// Returns the per-pool, per-slot capacities after reclamation.
    pub fn release(&self) -> [[usize; MAX_POOL_SLOTS]; N] {
        let mut capacities = [[0usize; MAX_POOL_SLOTS]; N];
        for (pool, out) in self.pools.iter().zip(capacities.iter_mut()) {
            *out = pool.release();
        }
        capacities
    }

    /// Clears every pool's recorded provisions so a later
    /// [`release`](Self::release) reclaims every slot entirely.
    /// See [`ScratchPool::reset`].
    pub fn reset(&self) {
        for pool in &self.pools {
            pool.reset();
        }
    }

    /// Returns the primary capacity for slot `INDEX`.
    ///
    /// Forwards to [`ScratchPool::capacity`] and inherits its contract: the
    /// figure is readable at any time, including from inside a live
    /// [`Self::with_scratch`] borrow of the same slot.
    ///
    /// # Panics
    ///
    /// Panics when `INDEX >= N`.
    #[inline]
    pub fn capacity<const INDEX: usize>(&self) -> usize {
        assert!(INDEX < N, "ScratchBank slot index out of range");
        self.pools[INDEX].capacity()
    }

    /// Returns the current borrow depth for slot `INDEX`.
    ///
    /// # Panics
    ///
    /// Panics when `INDEX >= N`.
    #[inline]
    pub fn borrow_depth<const INDEX: usize>(&self) -> u8 {
        assert!(INDEX < N, "ScratchBank slot index out of range");
        self.pools[INDEX].borrow_depth()
    }
}
