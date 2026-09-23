//! Aligned, grow-only scratch vector whose newly grown range is zeroed so a
//! reused buffer never exposes stale data.

mod bytes;
mod convert;
mod element_ops;
mod iter;
mod length;
mod query;
mod storage;
mod traits;

pub use iter::IntoIter;
pub use storage::AlignedVec;

use super::element::ScratchElement;

pub use length::Drain;
