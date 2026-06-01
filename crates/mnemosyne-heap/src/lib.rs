#![no_std]

extern crate alloc as std_alloc;

pub mod heap;
pub mod brand;
pub mod branded_heap;
pub mod branded_box;
pub mod branded_vec;

#[cfg(test)]
mod tests;

pub use heap::MnemosyneHeap;
pub use brand::{Invariant, AllocatorToken, BrandedBlock, BrandedCell, scope};
pub use branded_heap::BrandedHeap;
pub use branded_box::BrandedBox;
pub use branded_vec::BrandedVec;
