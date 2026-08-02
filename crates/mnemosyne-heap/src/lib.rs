//! Heap handles and tiered placement backends for Mnemosyne.
//!
//! Two concerns live here. [`Heap`] is the owning allocator handle, and
//! [`TieredHeap`] extends it with placement across memory tiers
//! ([`MemoryTier`], [`PlacementHint`]) so a caller can steer an allocation
//! toward the tier that matches its access pattern rather than treating
//! all memory as uniform.
//!
//! The `brand` family ([`BrandedBox`], [`BrandedVec`], [`BrandedBlock`])
//! carries an invariant lifetime that ties an allocation to the [`scope`]
//! it came from. Because the brand is invariant, a block cannot be
//! returned to a heap other than the one that produced it: the mismatch is
//! a type error rather than a runtime check, which is what keeps the
//! `no_std` free path free of validation.

#![no_std]
#![deny(missing_docs)]

extern crate alloc as std_alloc;

/// Invariant-lifetime brands binding an allocation to its originating
/// scope, so cross-heap returns are a compile error.
pub mod brand;
/// Single-value owning handle for a branded allocation.
pub mod branded_box;
/// Growable branded sequence.
pub mod branded_vec;
/// The owning allocator handle and its reallocation contract.
pub mod heap;
pub(crate) mod raw_heap;
pub mod tier;
pub mod tiered_backend;
pub mod tiered_heap;

#[cfg(test)]
mod tests;

pub use brand::{BrandedBlock, BrandedCell, InvariantLifetime, ThreadLocalToken, scope};
pub use branded_box::BrandedBox;
pub use branded_vec::BrandedVec;
pub use heap::{Heap, ReallocError, ReallocFailure};
pub use tier::{MemoryTier, PlacementHint};
pub use tiered_backend::TieredBackend;
pub use tiered_heap::{TieredBlock, TieredHeap, TieredReallocError, scope_tiered};
