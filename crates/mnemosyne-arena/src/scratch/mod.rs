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
pub use element::{DEFAULT_SCRATCH_ALIGN, ScratchElement, default_align};
pub use pool::{MAX_POOL_SLOTS, ScratchPool};
