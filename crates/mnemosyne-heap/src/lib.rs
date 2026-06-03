#![no_std]

extern crate alloc as std_alloc;

pub mod brand;
pub mod branded_box;
pub mod branded_vec;
pub mod heap;
pub(crate) mod raw_heap;

#[cfg(test)]
mod tests;

pub use brand::{scope, BrandedBlock, BrandedCell, InvariantLifetime, ThreadLocalToken};
pub use branded_box::BrandedBox;
pub use branded_vec::BrandedVec;
pub use heap::Heap;
