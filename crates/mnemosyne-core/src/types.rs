//! Core memory layout types: Block, Page, and Segment.

pub mod block;
pub mod owner;
pub mod page;
pub mod segment;
#[cfg(test)]
mod tests;

pub use block::Block;
pub use owner::SegmentOwner;
pub use page::Page;
pub use segment::Segment;
