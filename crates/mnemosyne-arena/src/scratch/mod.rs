//! Temporal aligned scratch pool for high-performance numerical workloads.
//!
//! FFT and transform workloads (e.g. Apollo) repeatedly need large, aligned
//! temporary buffers for Stockham autosort, Bluestein chirp, PFA scratch, and
//! Rader convolution. These buffers are typically allocated once, grown to the
//! maximum needed size, and reused across many transform calls.

pub mod aligned_vec;
pub mod bank;
pub mod element;
pub mod pool;

#[cfg(test)]
mod tests;

pub use aligned_vec::AlignedVec;
pub use bank::ScratchBank;
pub use element::{default_align, ScratchElement, DEFAULT_SCRATCH_ALIGN};
pub use pool::{ScratchPool, MAX_POOL_SLOTS};
