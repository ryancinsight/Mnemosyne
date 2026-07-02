#![no_std]

extern crate alloc as std_alloc;

pub mod brand;
pub mod branded_box;
pub mod branded_vec;
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
pub use heap::Heap;
pub use tier::{MemoryTier, PlacementHint};
pub use tiered_backend::TieredBackend;
pub use tiered_heap::{TieredBlock, TieredHeap, scope_tiered};
