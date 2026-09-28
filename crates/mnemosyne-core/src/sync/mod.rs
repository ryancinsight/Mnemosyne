//! Lock-free synchronization primitives for the allocator.
//!
//! The primary type is [`AtomicFreeList`]: a lock-free singly-linked list used
//! as the cross-thread deallocation queue on each page.
//!
//! # One implementation, two head encodings
//!
//! [`AtomicFreeList`] has a single implementation, in [`free_list`]. The only
//! target-dependent part is how its head word is encoded, isolated behind the
//! ZST `HeadCodec` and selected by a private per-module `SelectedHead` alias:
//!
//! - `sync/wide.rs` — 64-bit `PackedHead`: packs `(head_addr, push_counter)`
//!   into one `AtomicPtr` word, so a drain reads the count in O(1).
//! - `sync/narrow.rs` — 32-bit `BareHead`: a bare `AtomicPtr` head whose count
//!   comes from an O(k) chain walk.
//!
//! The push CAS loop, the double-free head check, and the draining swap are
//! written once against the codec, so the two encodings cannot drift apart.

mod free_list;
#[cfg(not(target_pointer_width = "64"))]
mod narrow;
mod packed;
#[cfg(target_pointer_width = "64")]
mod wide;

pub use free_list::AtomicFreeList;
pub(crate) use free_list::HeadCodec;
pub use packed::PackedTaggedPtr;
