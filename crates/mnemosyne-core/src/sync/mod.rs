//! Lock-free synchronization primitives for the allocator.
//!
//! The primary type is [`AtomicFreeList`]: a lock-free singly-linked list used
//! as the cross-thread deallocation queue on each page.
//!
//! # Platform-specific implementations
//!
//! Both submodules contribute `impl AtomicFreeList` blocks; the struct itself
//! is defined here. Rust permits multiple `impl` blocks across files.
//!
//! - `sync/wide.rs` — 64-bit packed `(head_addr, push_counter)` in one
//!   `AtomicPtr`; `pop_all` is O(1).
//! - `sync/narrow.rs` — 32-bit fallback with a bare `AtomicPtr` head; count
//!   comes from an O(k) chain walk.

use crate::loom_shim::AtomicPtr;

#[cfg(not(target_pointer_width = "64"))]
mod narrow;
#[cfg(target_pointer_width = "64")]
mod wide;

/// A lock-free, atomic singly-linked list of blocks.
///
/// Implements atomic push and atomic pop-all operations, matching the
/// deallocation queue pattern from mimalloc. The 64-bit implementation packs a
/// push counter into the high bits of the head pointer so pop_all can return
/// the block count in O(1); the 32-bit fallback stores a bare pointer and walks
/// the chain.
pub struct AtomicFreeList {
    pub(crate) head: AtomicPtr<crate::types::Block>,
}

impl Default for AtomicFreeList {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
