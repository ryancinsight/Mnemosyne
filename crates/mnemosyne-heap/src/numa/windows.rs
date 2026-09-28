//! Windows NUMA primitives: `VirtualAllocExNuma` chunked per-node commit
//! for interleave allocation. Node binding of existing allocations has no
//! Windows equivalent and is served by the [`super::fallback`] no-op.

use super::{FIRST_TOUCH_STRIDE, NumaError};
use core::alloc::Layout;
use core::ptr::NonNull;

/// Windows implementation of [`super::allocate_interleaved`].
///
/// Uses `VirtualAllocExNuma` with per-node chunked commits when the topology
/// reports more than one node, matching the pre-existing consumer contract;
/// falls back to a plain allocation otherwise. The multi-node path returns
/// memory that must be released with `VirtualFree(MEM_RELEASE)`.
///
/// # Errors
///
/// Returns [`NumaError::Allocation`] when the host allocation itself fails.
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
