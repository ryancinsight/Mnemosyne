//! Temporal aligned scratch pool for high-performance numerical workloads.
//!
//! FFT and transform workloads (e.g. Apollo) repeatedly need large, aligned
//! temporary buffers for Stockham autosort, Bluestein chirp, PFA scratch, and
//! Rader convolution. These buffers are typically allocated once, grown to the
//! maximum needed size, and reused across many transform calls.

pub mod element;
pub mod aligned_vec;
pub mod pool;
pub mod bank;

#[cfg(test)]
mod tests;

pub use element::{ScratchElement, DEFAULT_SCRATCH_ALIGN, default_align};
pub use aligned_vec::AlignedVec;
pub use pool::{ScratchPool, MAX_POOL_SLOTS};
pub use bank::ScratchBank;
