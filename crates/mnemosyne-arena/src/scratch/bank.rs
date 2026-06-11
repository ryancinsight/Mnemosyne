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
