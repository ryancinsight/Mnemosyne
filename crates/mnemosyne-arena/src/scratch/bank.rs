//! Scratch bank implementation for keeping multiple related pools together.

use super::element::ScratchElement;
use super::pool::ScratchPool;

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

    /// Returns the primary capacity for slot `INDEX`.
    ///
    /// Forwards to [`ScratchPool::capacity`] and inherits its contract: the
    /// figure is readable at any time, including from inside a live
    /// [`Self::with_scratch`] borrow of the same slot.
    ///
    /// # Panics
    /// Frees every pool's slot allocations and reports the total bytes
    /// released, or [`None`] if any pool has a live borrow.
    ///
    /// The bank-wide counterpart to [`ScratchPool::release`]. A consumer that
    /// holds one bank per worker thread — several roles, each grown to its own
    /// high-water mark — retains the sum of those marks for the life of the
    /// thread. This releases all of them at a quiescent point of the caller's
    /// choosing.
    ///
    /// All-or-nothing: if any pool is mid-borrow, nothing is freed, so a caller
    /// that reaches this from inside a `with_scratch` closure cannot
    /// half-release the bank underneath itself.
    ///
    /// # Examples
    ///
    /// ```
    /// use mnemosyne_arena::scratch::ScratchBank;
    ///
    /// let bank: ScratchBank<f64, 2> = ScratchBank::new();
    /// bank.with_scratch::<0, _>(512, |s| s[0] = 1.0);
    /// bank.with_scratch::<1, _>(256, |s| s[0] = 2.0);
    ///
    /// let freed = bank.release().expect("no borrow is live here");
    /// assert!(freed >= (512 + 256) * size_of::<f64>());
    /// assert_eq!(bank.capacity::<0>(), 0);
    /// ```
    #[must_use]
    pub fn release(&self) -> Option<usize> {
        if self.pools.iter().any(|pool| pool.borrow_depth() != 0) {
            return None;
        }
        let mut freed = 0_usize;
        for pool in &self.pools {
            // The depth check above already established that every pool is
            // quiescent, so no pool can refuse here.
            let released = pool
                .release()
                .expect("invariant: every pool was checked quiescent above");
            freed = freed.saturating_add(released);
        }
        Some(freed)
    }

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
