//! Temporal aligned scratch pool for high-performance numerical workloads.
//!
//! FFT and transform workloads (e.g. Apollo) repeatedly need large, aligned
//! temporary buffers for Stockham autosort, Bluestein chirp, PFA scratch, and
//! Rader convolution. These buffers are typically allocated once, grown to the
//! maximum needed size, and reused across many transform calls.
//!
//! Two complementary buffer types cover the full size range:
//!
//! | Type | Storage | Heap | Use when |
//! |------|---------|------|----------|
//! | [`AlignedVec<T>`] | heap | yes | size unknown at compile time |
//! | [`AlignedBuf<T, N>`] | stack | **no** | size bounded at compile time |

pub mod aligned_buf;
pub mod aligned_vec;
pub mod bank;
pub mod element;
pub mod pool;

#[cfg(test)]
mod tests;

pub use aligned_buf::AlignedBuf;
pub use aligned_vec::{AlignedVec, Drain, IntoIter};
pub use bank::ScratchBank;
pub use element::{DEFAULT_SCRATCH_ALIGN, ScratchElement, default_align};
pub use pool::{MAX_POOL_SLOTS, ScratchPool};
