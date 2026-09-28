//! NUMA-node binding, interleave allocation, and first-touch primitives.
//!
//! The execution counterpart of [`themis::PlacementHint::Numa`]: themis owns
//! the placement *vocabulary* ([`themis::NumaNodeId`], topology detection,
//! hint types) and this module owns the *execution* — the kernel
//! memory-policy calls that make a hint real. Consumers that manage raw
//! allocations (arena segment pools, SoA field buffers) call these
//! primitives directly; [`crate::tiered_heap::TieredHeap::alloc`] routes
//! `PlacementHint::Numa(node)` through [`bind_to_node`] internally.
//!
//! The primitives are split by platform so each leaf owns one target's
//! execution path (each is `#[cfg]`-gated to its target, so at most one is
//! compiled in for any given build):
//! - `linux` — full support: `bind_to_node` issues `mbind(MPOL_BIND)` and
//!   `allocate_interleaved` issues `mbind(MPOL_INTERLEAVE)` after a standard
//!   allocation.
//! - `windows` — `allocate_interleaved` uses `VirtualAllocExNuma` when the
//!   topology reports more than one node (chunked per-node commit) and falls
//!   back to a plain allocation otherwise.
//! - `fallback` — a `bind_to_node` no-op (Windows and other targets, which
//!   have no `mbind` equivalent for existing allocations) and a plain
//!   `allocate_interleaved` on targets without the Windows path.
//!
//! [`first_touch`] is cross-platform and realizes page placement on the
//! faulting thread; it lives here alongside the shared [`NumaError`] type.
//!
//! Binding is *best-effort* by contract: every caller in the stack treats a
//! failed policy call as a locality hint that could not be honored, never as
//! an allocation failure. The error type exists so explicit callers can
//! distinguish and log the reason.
//!
//! # Examples
//!
//! ```
//! use core::alloc::Layout;
//! use mnemosyne_heap::numa::{bind_to_node, first_touch};
//! use themis::NumaNodeId;
//!
//! let layout = Layout::from_size_align(4096, 4096).unwrap();
//! // SAFETY: a fresh `std::alloc` allocation, released below.
//! let ptr = unsafe { std::alloc::alloc(layout) };
//! if !ptr.is_null() {
//!     // SAFETY: `ptr` is a live allocation of `layout.size()` bytes; a
//!     // failed policy call is a best-effort hint, not an error here.
//!     let _ = unsafe { bind_to_node(ptr, layout.size(), NumaNodeId::ZERO) };
//!     // SAFETY: same range, still live and writable.
//!     unsafe { first_touch(ptr, layout.size()) };
//!     // SAFETY: deallocate exactly the allocation `ptr` came from.
//!     unsafe { std::alloc::dealloc(ptr, layout) };
//! }
//! ```

#[cfg(not(target_os = "linux"))]
mod fallback;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub use fallback::allocate_interleaved;
#[cfg(not(target_os = "linux"))]
pub use fallback::bind_to_node;
#[cfg(target_os = "linux")]
pub use linux::{allocate_interleaved, bind_to_node};
#[cfg(target_os = "windows")]
pub use windows::allocate_interleaved;

use themis::NumaNodeId;

/// Stride used by [`first_touch`] to walk an allocation page by page.
///
/// 4096 is the smallest OS page size across every supported target (x86_64,
/// aarch64, Windows). Because the stride divides every larger page size
/// (16 KiB, 64 KiB), striding at 4096 bytes touches *at least once* every
/// OS page of an allocation, regardless of the host page size or the
/// allocation's alignment — touching a page more than once is harmless for
/// first-touch placement.
pub(super) const FIRST_TOUCH_STRIDE: usize = 4096;

/// Failure modes for NUMA placement operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NumaError {
    /// The kernel rejected a placement system call.
    Syscall {
        /// The `errno` reported by the kernel.
        errno: i32,
        /// The system call that failed (for diagnostics).
        op: &'static str,
    },
    /// The underlying host allocation failed.
    Allocation {
        /// The number of bytes that could not be allocated.
        requested_bytes: usize,
    },
    /// The node identifier falls outside the range the kernel nodemask can
    /// express.
    InvalidNode {
        /// The offending node identifier.
        node: NumaNodeId,
    },
}

/// Touches every page of an allocation to realize first-touch placement.
///
/// Writing a single byte to each page forces the kernel to fault the page
/// in on the calling thread, which is what the OS first-touch policy uses
/// to place the page on the node that accessed it. The touch is a volatile
/// write so the optimizer cannot elide it.
///
/// The stride (4096 bytes) is at most the OS page size on every supported
/// target, so each OS page receives at least one touch regardless of host
/// page size or allocation alignment.
///
/// # Safety
///
/// `ptr` must be valid for `size` bytes and the memory must be writable for
/// the duration of the call.
pub unsafe fn first_touch(ptr: *mut u8, size: usize) {
    let mut offset = 0usize;
    while offset < size {
        // SAFETY: `offset` is bounded by `size`, so `ptr.add(offset)` stays
        // within the caller-guaranteed valid range; the volatile write only
        // touches the first byte of the page.
        unsafe { core::ptr::write_volatile(ptr.add(offset), 0u8) };
        offset += FIRST_TOUCH_STRIDE;
    }
}
