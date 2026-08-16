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
//! Platform contract:
//! - Linux — full support: [`bind_to_node`] issues `mbind(MPOL_BIND)`,
//!   [`allocate_interleaved`] issues `mbind(MPOL_INTERLEAVE)` after a
//!   standard allocation, and [`first_touch`] realizes page placement.
//! - Windows — [`allocate_interleaved`] uses `VirtualAllocExNuma` when the
//!   topology reports more than one node (chunked per-node commit) and falls
//!   back to a plain allocation otherwise; [`bind_to_node`] is a documented
//!   no-op because Windows has no `mbind` equivalent for existing
//!   allocations.
//! - Other platforms — plain allocation and a [`bind_to_node`] no-op.
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

use core::alloc::Layout;
use core::ptr::NonNull;
use themis::NumaNodeId;

/// Stride used by [`first_touch`] to walk an allocation page by page.
///
/// 4096 is the smallest OS page size across every supported target (x86_64,
/// aarch64, Windows). Because the stride divides every larger page size
/// (16 KiB, 64 KiB), striding at 4096 bytes touches *at least once* every
/// OS page of an allocation, regardless of the host page size or the
/// allocation's alignment — touching a page more than once is harmless for
/// first-touch placement.
const FIRST_TOUCH_STRIDE: usize = 4096;

/// Upper bound on the number of NUMA nodes expressible in the kernel
/// `nodemask` argument of `mbind`.
///
/// `mbind` takes `maxnode` as a count of bits; 1024 matches the kernel's
/// `MAX_NUMNODES` for the common `CONFIG_NODES_SHIFT` configurations and is
/// the same bound the pre-existing consumer implementation used.
#[cfg(target_os = "linux")]
const MAX_NUMA_NODES: usize = 1024;

/// Kernel memory-policy constants (`linux/mempolicy.h`), kept local because
/// `libc` does not expose them on every supported architecture.
#[cfg(target_os = "linux")]
mod linux_policy {
    /// `MPOL_BIND` — bind memory to the nodes in the mask.
    pub const MPOL_BIND: i32 = 2;
    /// `MPOL_INTERLEAVE` — interleave memory across the nodes in the mask.
    pub const MPOL_INTERLEAVE: i32 = 3;
    /// `MPOL_MF_STRICT` — fail if a page in the range cannot honor the policy.
    pub const MPOL_MF_STRICT: u32 = 1;
}

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

/// Binds an existing allocation to a NUMA node (`mbind(MPOL_BIND)` on
/// Linux; a documented no-op elsewhere).
///
/// The kernel rounds the range to page boundaries, so the effective range
/// may extend slightly beyond `ptr..ptr + size`; callers that require exact
/// page semantics should pass page-aligned pointers and page-multiple sizes.
///
/// # Safety
///
/// `ptr` must be a valid allocation of at least `size` bytes that remains
/// live for the duration of the call. The memory is not dereferenced — the
/// kernel only sets a memory policy on the range — but binding a range that
/// is not the caller's own allocation would silently attach a policy to
/// foreign pages.
///
/// # Errors
///
/// Returns [`NumaError::Syscall`] when the kernel rejects the policy call
/// (for example a node id outside the topology, or `MPOL_MF_STRICT` finding
/// pages that cannot be rebound). Binding is best-effort: callers should
/// treat an error as a locality hint that could not be honored.
#[cfg(target_os = "linux")]
pub unsafe fn bind_to_node(ptr: *mut u8, size: usize, node: NumaNodeId) -> Result<(), NumaError> {
    let node_usize = usize::from(node.get());
    if node_usize >= MAX_NUMA_NODES {
        return Err(NumaError::InvalidNode { node });
    }
    let mut nodemask = [0u64; MAX_NUMA_NODES.div_ceil(64)];
    nodemask[node_usize / 64] |= 1u64 << (node_usize % 64);

    // SAFETY: `mbind` takes the nodemask by pointer only for the duration of
    // the call, and `ptr` satisfies the caller-provided validity contract.
    // The range is rounded to pages by the kernel; `MPOL_MF_STRICT` turns an
    // unhonorable range into an `EIO` rather than a silent partial policy.
    let result = unsafe {
        libc::syscall(
            libc::SYS_mbind,
            ptr,
            size,
            linux_policy::MPOL_BIND,
            nodemask.as_ptr(),
            MAX_NUMA_NODES,
            linux_policy::MPOL_MF_STRICT,
        )
    };

    if result < 0 {
        // SAFETY: `__errno_location` is valid for the current thread for the
        // duration of the call.
        let errno = unsafe { *libc::__errno_location() };
        return Err(NumaError::Syscall { errno, op: "mbind" });
    }
    Ok(())
}

/// Binds an existing allocation to a NUMA node.
///
/// # Safety
///
/// See the Linux implementation's safety contract. On platforms without
/// node binding the call is a documented no-op that always succeeds.
///
/// # Errors
///
/// Always returns `Ok` on platforms without node binding.
#[cfg(not(target_os = "linux"))]
pub unsafe fn bind_to_node(
    _ptr: *mut u8,
    _size: usize,
    _node: NumaNodeId,
) -> Result<(), NumaError> {
    Ok(())
}

/// Allocates memory interleaved across the topology's NUMA nodes.
///
/// Linux: a standard allocation followed by a best-effort
/// `mbind(MPOL_INTERLEAVE)` over every node the topology reports, so the
/// kernel distributes the pages round-robin as they are faulted. Windows:
/// `VirtualAllocExNuma` with per-node chunked commits when the topology
/// reports more than one node; a plain allocation otherwise. Other
/// platforms: a plain allocation.
///
/// The returned pointer must be released with the matching deallocator:
/// `std_alloc::alloc::dealloc` (Linux / other) or `VirtualFree`
/// (Windows multi-node path).
///
/// # Errors
///
/// Returns [`NumaError::Allocation`] when the host allocation itself fails.
/// The interleave policy call is best-effort and never turns a successful
/// allocation into an error.
#[cfg(target_os = "linux")]
pub fn allocate_interleaved(layout: Layout) -> Result<NonNull<u8>, NumaError> {
    // SAFETY: `Layout` is a valid allocation layout by construction
    // (nonzero size, power-of-two alignment); `alloc` returns null on
    // failure, which is checked below.
    let ptr = unsafe { std_alloc::alloc::alloc(layout) };
    let Some(non_null) = NonNull::new(ptr) else {
        return Err(NumaError::Allocation {
            requested_bytes: layout.size(),
        });
    };

    if let Some(topology) = themis::CpuTopology::detect() {
        let mut nodemask = [0u64; MAX_NUMA_NODES.div_ceil(64)];
        for node in topology.numa_nodes() {
            let idx = usize::from(node.id.get());
            if idx < MAX_NUMA_NODES {
                nodemask[idx / 64] |= 1u64 << (idx % 64);
            }
        }
        // SAFETY: `ptr` is a valid allocation of `layout.size()` bytes; the
        // kernel only sets a policy on the range and never dereferences the
        // pointer through this API. The result is deliberately ignored —
        // an unhonorable interleave policy leaves a usable allocation.
        let _ = unsafe {
            libc::syscall(
                libc::SYS_mbind,
                ptr,
                layout.size(),
                linux_policy::MPOL_INTERLEAVE,
                nodemask.as_ptr(),
                MAX_NUMA_NODES,
                0u32,
            )
        };
    }

    Ok(non_null)
}

/// Windows implementation of [`allocate_interleaved`].
///
/// Uses `VirtualAllocExNuma` with per-node chunked commits when the topology
/// reports more than one node, matching the pre-existing consumer contract;
/// falls back to a plain allocation otherwise. The multi-node path returns
/// memory that must be released with `VirtualFree(MEM_RELEASE)`.
#[cfg(target_os = "windows")]
pub fn allocate_interleaved(layout: Layout) -> Result<NonNull<u8>, NumaError> {
    mod win_numa {
        use core::ffi::c_void;

        unsafe extern "system" {
            pub fn VirtualAllocExNuma(
                h_process: *mut c_void,
                lp_address: *mut c_void,
                dw_size: usize,
                fl_allocation_type: u32,
                fl_protect: u32,
                nnd_preferred: u32,
            ) -> *mut c_void;
            pub fn VirtualFree(lp_address: *mut c_void, dw_size: usize, dw_free_type: u32) -> i32;
            pub fn GetCurrentProcess() -> *mut c_void;
        }

        pub const MEM_COMMIT: u32 = 0x1000;
        pub const MEM_RESERVE: u32 = 0x2000;
        pub const MEM_RELEASE: u32 = 0x8000;
        pub const PAGE_READWRITE: u32 = 0x04;
    }

    let nodes = themis::CpuTopology::detect().map_or(0, |topology| topology.numa_nodes().len());

    if nodes <= 1 {
        // SAFETY: `Layout` is valid by construction; null means failure.
        let ptr = unsafe { std_alloc::alloc::alloc(layout) };
        return NonNull::new(ptr).ok_or(NumaError::Allocation {
            requested_bytes: layout.size(),
        });
    }

    let size = layout.size();
    let chunk_size = (size / nodes).max(FIRST_TOUCH_STRIDE);

    // SAFETY: `GetCurrentProcess` returns the pseudo-handle of the calling
    // process; it is a constant that never fails and needs no release.
    let process = unsafe { win_numa::GetCurrentProcess() };

    // SAFETY: `VirtualAllocExNuma` with a null address asks the kernel to
    // pick a region; null return means failure, checked below.
    let base_ptr = unsafe {
        win_numa::VirtualAllocExNuma(
            process,
            core::ptr::null_mut(),
            size,
            win_numa::MEM_RESERVE,
            win_numa::PAGE_READWRITE,
            0,
        )
    };

    if base_ptr.is_null() {
        return Err(NumaError::Allocation {
            requested_bytes: size,
        });
    }

    let mut offset = 0usize;
    let mut current_node = 0usize;
    while offset < size {
        let commit_size = chunk_size.min(size - offset);
        // SAFETY: `offset` is bounded by `size` and `base_ptr` is a valid
        // reserved region of `size` bytes, so the chunk pointer is in range.
        let chunk_ptr = unsafe { base_ptr.add(offset) };

        // SAFETY: committing a sub-range of the reserved region with
        // `MEM_COMMIT` is the documented Windows two-phase allocation
        // sequence; null return means failure.
        let result = unsafe {
            win_numa::VirtualAllocExNuma(
                process,
                chunk_ptr,
                commit_size,
                win_numa::MEM_COMMIT,
                win_numa::PAGE_READWRITE,
                current_node as u32,
            )
        };

        if result.is_null() {
            // SAFETY: `base_ptr` is the region reserved above and
            // `MEM_RELEASE` releases the whole region.
            unsafe { win_numa::VirtualFree(base_ptr, 0, win_numa::MEM_RELEASE) };
            return Err(NumaError::Allocation {
                requested_bytes: size,
            });
        }

        offset += commit_size;
        current_node = (current_node + 1) % nodes;
    }

    // SAFETY: `base_ptr` is non-null and the region is fully committed.
    unsafe { Ok(NonNull::new_unchecked(base_ptr.cast::<u8>())) }
}

/// Other-platform implementation of [`allocate_interleaved`].
///
/// A plain allocation: platforms without a node-interleave mechanism get
/// default placement, which the OS realizes via first-touch.
///
/// # Errors
///
/// Returns [`NumaError::Allocation`] when the host allocation fails.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn allocate_interleaved(layout: Layout) -> Result<NonNull<u8>, NumaError> {
    // SAFETY: `Layout` is valid by construction; null means failure.
    let ptr = unsafe { std_alloc::alloc::alloc(layout) };
    NonNull::new(ptr).ok_or(NumaError::Allocation {
        requested_bytes: layout.size(),
    })
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
