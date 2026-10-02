//! Segment metadata: the fixed-size mapping that owns a run of pages.
//!
//! Responsibility is split across focused submodules (each private; their
//! public items are re-exported here):
//! - `location` — `expose_mapping` and the `locate_segment` / `locate_page` /
//!   `locate_huge_back_pointer` address-arithmetic helpers.
//! - `freelist` — cookie derivation, mode validation, encryption-flag read.
//! - `access` — ownership and current-slicing raw-pointer accessors.
//! - `ownership` — the `SegmentOwnership` atomic pair type.
//! - `header` — the `Segment` struct shape, its `Send`/`Sync` impls,
//!   `Default`, and the `initialize` / `huge_mapping_suffix_from` lifecycle
//!   methods.

mod access;
mod freelist;
mod header;
mod location;
mod ownership;

pub use access::OccupiedPageBits;
pub use header::Segment;
pub use location::{
    expose_mapping, locate_block, locate_huge_back_pointer, locate_page, locate_segment,
};
pub use ownership::SegmentOwnership;
