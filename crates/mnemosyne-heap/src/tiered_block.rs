//! Tier-carrying block handles and reallocation failures shared by
//! [`crate::tiered_heap::TieredHeap`] for any [`mnemosyne_core::AllocPolicy`].

use crate::brand::BrandedBlock;
use crate::heap::{ReallocError, ReallocFailure};
use crate::tier::MemoryTier;

/// A [`BrandedBlock`] enriched with the [`MemoryTier`] it was allocated
/// against.
///
/// The tier field is the only place the tier lives once an allocation
/// has happened — the underlying `BrandedBlock` does not know its pool,
/// and the heap façade needs the tier at `free` time to return the
/// memory to the right sub-heap.
pub struct TieredBlock<'brand, T: ?Sized> {
    pub(crate) block: BrandedBlock<'brand, T>,
    pub(crate) tier: MemoryTier,
}

impl<'brand, T: ?Sized> TieredBlock<'brand, T> {
    /// Returns the [`MemoryTier`] this block was allocated against.
    ///
    /// Same value as [`crate::tier::tier_for`] applied to the original
    /// `PlacementHint`; carried alongside the block so cross-tier free
    /// can route back to the right pool without re-passing the hint.
    #[inline(always)]
    #[must_use]
    pub fn tier(&self) -> MemoryTier {
        self.tier
    }

    /// Returns the raw pointer to the block's managed memory.
    ///
    /// Convenience accessor matching [`BrandedBlock::as_ptr`].
    #[inline(always)]
    #[must_use]
    pub fn as_ptr(&self) -> *mut T {
        self.block.as_ptr()
    }
}

/// A failed tiered reallocation that retains the source block and its tier.
///
/// Call [`Self::into_block`] to recover the source allocation after inspecting
/// [`Self::reason`]. The error is an ownership handle: callers must either
/// recover the block or explicitly release it through the owning heap.
#[must_use]
pub struct TieredReallocError<'brand, T: ?Sized> {
    pub(crate) block: TieredBlock<'brand, T>,
    pub(crate) reason: ReallocFailure,
}

impl<'brand, T: ?Sized> TieredReallocError<'brand, T> {
    /// Returns the failure classification without consuming the source block.
    #[inline]
    #[must_use]
    pub fn reason(&self) -> ReallocFailure {
        self.reason
    }

    /// Recovers the original source block without dropping or deallocating it.
    #[inline]
    #[must_use]
    pub fn into_block(self) -> TieredBlock<'brand, T> {
        self.block
    }
}

impl<'brand, T: ?Sized> core::fmt::Debug for TieredReallocError<'brand, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TieredReallocError")
            .field("block", &self.block.as_ptr())
            .field("tier", &self.block.tier)
            .field("reason", &self.reason)
            .finish()
    }
}

pub(crate) fn map_realloc_result<'brand, T: ?Sized>(
    tier: MemoryTier,
    result: Result<Option<BrandedBlock<'brand, u8>>, ReallocError<'brand, T>>,
) -> Result<Option<TieredBlock<'brand, u8>>, TieredReallocError<'brand, T>> {
    match result {
        Ok(block) => Ok(block.map(|block| TieredBlock { block, tier })),
        Err(error) => {
            let reason = error.reason();
            let block = error.into_block();
            Err(TieredReallocError {
                block: TieredBlock { block, tier },
                reason,
            })
        }
    }
}
