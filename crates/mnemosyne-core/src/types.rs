//! Core memory layout types: Block, Page, and Segment.

pub mod block;
pub mod owner;
pub mod page;
pub mod segment;
#[cfg(test)]
mod tests;

pub use block::Block;
pub use owner::SegmentOwner;
#[cfg(all(windows, target_arch = "x86_64", not(miri)))]
pub use owner::current_thread_id;
pub use page::Page;
pub use segment::{Segment, locate_page, locate_segment};
