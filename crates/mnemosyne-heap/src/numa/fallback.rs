//! Fallback NUMA primitives for targets without kernel node-placement.
//!
//! `bind_to_node` is a documented no-op on every non-Linux target (Windows
//! included, which has no `mbind` equivalent for existing allocations).
//! `allocate_interleaved` is a plain allocation on targets that also lack
//! the Windows `VirtualAllocExNuma` path; the OS realizes placement via
//! first-touch.

use super::NumaError;
use themis::NumaNodeId;

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
pub unsafe fn bind_to_node(
    _ptr: *mut u8,
    _size: usize,
    _node: NumaNodeId,
) -> Result<(), NumaError> {
    Ok(())
}

/// Other-platform implementation of [`super::allocate_interleaved`].
///
/// A plain allocation: platforms without a node-interleave mechanism get
/// default placement, which the OS realizes via first-touch.
///
/// # Errors
///
/// Returns [`NumaError::Allocation`] when the host allocation fails.
#[cfg(not(target_os = "windows"))]
pub fn allocate_interleaved(
    layout: core::alloc::Layout,
) -> Result<core::ptr::NonNull<u8>, NumaError> {
    // SAFETY: `Layout` is valid by construction; null means failure.
    let ptr = unsafe { std_alloc::alloc::alloc(layout) };
    core::ptr::NonNull::new(ptr).ok_or(NumaError::Allocation {
        requested_bytes: layout.size(),
    })
}
