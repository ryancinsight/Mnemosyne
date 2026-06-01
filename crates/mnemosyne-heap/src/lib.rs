#![no_std]

extern crate alloc as std_alloc;

pub mod brand;
pub mod branded_box;
pub mod branded_heap;
pub mod branded_vec;
pub mod heap;
pub(crate) mod raw_heap;

#[cfg(test)]
mod tests;

pub use brand::{scope, AllocatorToken, BrandedBlock, BrandedCell, Invariant};
pub use branded_box::BrandedBox;
pub use branded_heap::BrandedHeap;
pub use branded_vec::BrandedVec;
pub use heap::MnemosyneHeap;
