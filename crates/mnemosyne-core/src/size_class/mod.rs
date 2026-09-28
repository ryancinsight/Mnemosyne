//! Size class calculations and mapping.
//!
//! The module is organized by concern:
//! - [`tables`] — SSOT compile-time lookup tables (`CLASS_TO_SIZE`,
//!   `CLASS_TO_MAX_BLOCKS`, `CLASS_TO_DIV_MULT`, `SIZE_TO_CLASS`) and their
//!   O(1) direct accessors (`class_to_size`, `class_to_max_blocks`,
//!   `block_index_in_page`).
//! - [`arithmetic`] — the `const fn` piecewise bucketing math that drives the
//!   reverse table at compile time (`size_to_class_nonzero_arithmetic`).
//! - [`info`] — `SizeClassInfo` compile-time metadata struct.
//! - [`query`] — the functions that combine the reverse and forward tables:
//!   `size_to_class*`, `round_up_size*`, `size_class_fragmentation`, and
//!   their tests.

mod arithmetic;
mod info;
mod query;
pub mod tables;

pub use info::{SizeClassInfo, all_class_info};
pub use query::{
    round_up_size, round_up_size_saturating, size_class_fragmentation, size_to_class,
    size_to_class_nonzero,
};
pub use tables::{LEMIRE_DIV_SHIFT, block_index_in_page, class_to_max_blocks, class_to_size};
