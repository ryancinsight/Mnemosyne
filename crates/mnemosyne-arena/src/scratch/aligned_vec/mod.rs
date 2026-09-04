//! Aligned, grow-only scratch vector whose newly grown range is zeroed so a
//! reused buffer never exposes stale data.

mod bytes;
mod length;
mod storage;
mod traits;

pub use storage::AlignedVec;
pub use traits::IntoIter;

use super::element::ScratchElement;

pub use length::Drain;
