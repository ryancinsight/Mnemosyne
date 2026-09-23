//! Linux NUMA primitives: `mbind`-based node binding and interleave
//! allocation using the kernel memory-policy syscalls.

use super::NumaError;
use core::alloc::Layout;
use core::ptr::NonNull;
use themis::NumaNodeId;

/// Upper bound on the number of NUMA nodes expressible in the kernel
/// `nodemask` argument of `mbind`.
///
/// `mbind` takes `maxnode` as a count of bits; 1024 matches the kernel's
/// `MAX_NUMNODES` for the common `CONFIG_NODES_SHIFT` configurations and is
/// the same bound the pre-existing consumer implementation used.
const MAX_NUMA_NODES: usize = 1024;

/// Kernel memory-policy constants (`linux/mempolicy.h`), kept local because
/// `libc` does not expose them on every supported architecture.
mod linux_policy {
    /// `MPOL_BIND` — bind memory to the nodes in the mask.
    pub const MPOL_BIND: i32 = 2;
    /// `MPOL_INTERLEAVE` — interleave memory across the nodes in the mask.
    pub const MPOL_INTERLEAVE: i32 = 3;
    /// `MPOL_MF_STRICT` — fail if a page in the range cannot honor the policy.
    pub const MPOL_MF_STRICT: u32 = 1;
}

/// Binds an existing allocation to a NUMA node via `mbind(MPOL_BIND)`.
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
pub unsafe fn bind_to_node(ptr: *mut u8, size: usize, node: NumaNodeId) -> Result<(), NumaError> {
    let node_usize = node.get() as usize;
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

/// Allocates memory interleaved across the topology's NUMA nodes.
///
/// A standard allocation followed by a best-effort `mbind(MPOL_INTERLEAVE)`
/// over every node the topology reports, so the kernel distributes the pages
/// round-robin as they are faulted. The returned pointer must be released
/// with `std_alloc::alloc::dealloc`.
///
/// # Errors
///
/// Returns [`NumaError::Allocation`] when the host allocation itself fails.
/// The interleave policy call is best-effort and never turns a successful
/// allocation into an error.
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
            let idx = node.id.get() as usize;
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
