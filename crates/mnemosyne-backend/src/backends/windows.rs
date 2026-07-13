//! Windows VirtualAlloc/VirtualFree memory backend.

#[cfg(miri)]
use core::alloc::{GlobalAlloc, Layout};
#[cfg(not(miri))]
use core::ffi::c_void;
#[cfg(miri)]
use std::alloc::System;

#[cfg(miri)]
extern crate std;

// Windows API constants
#[cfg(not(miri))]
const MEM_COMMIT: u32 = 0x00001000;
#[cfg(not(miri))]
const MEM_RESERVE: u32 = 0x00002000;
#[cfg(not(miri))]
const MEM_RELEASE: u32 = 0x00008000;
/// `MEM_DECOMMIT` releases the physical/pagefile commitment of a page range
/// while keeping the address reservation, so a later `MEM_RELEASE` of the base
/// reservation still covers it. Unlike `MEM_RESET`, it drops commit charge.
#[cfg(not(miri))]
const MEM_DECOMMIT: u32 = 0x00004000;
/// `MEM_RESET` advises the Memory Manager that the addressed pages no
/// longer need to retain their contents; the OS may discard them and
/// re-zero on next access, but the mapping itself stays committed.
#[cfg(not(miri))]
const MEM_RESET: u32 = 0x00080000;
#[cfg(not(miri))]
const PAGE_READWRITE: u32 = 0x04;
/// `PAGE_NOACCESS` makes the page region raise an access-violation
/// fault on any read, write, or execute attempt while keeping the
/// mapping reserved.
#[cfg(not(miri))]
const PAGE_NOACCESS: u32 = 0x01;

#[cfg(not(miri))]
unsafe extern "system" {
    fn VirtualAlloc(
        lpAddress: *const c_void,
        dwSize: usize,
        flAllocationType: u32,
        flProtect: u32,
    ) -> *mut c_void;

    fn VirtualFree(lpAddress: *mut c_void, dwSize: usize, dwFreeType: u32) -> i32;

    fn VirtualProtect(
        lpAddress: *mut c_void,
        dwSize: usize,
        flNewProtect: u32,
        lpflOldProtect: *mut u32,
    ) -> i32;
}

/// Windows virtual memory backend using `VirtualAlloc`/`VirtualFree`.
pub struct WindowsBackend;

impl mnemosyne_core::MemoryBackend for WindowsBackend {
    const SUPPORTS_PAGE_RESET: bool = !cfg!(miri);
    const SUPPORTS_MAKE_GUARD: bool = !cfg!(miri);
    const SUPPORTS_DECOMMIT: bool = !cfg!(miri);

    /// Reserves and commits virtual memory pages of the given size.
    ///
    /// # Safety
    ///
    /// The size must be a multiple of the system page size (usually 4KB).
    unsafe fn allocate(size: usize) -> *mut u8 {
        #[cfg(miri)]
        {
            const OS_PAGE_ALIGN: usize = 4096;
            // SAFETY: callers guarantee a non-zero page-size multiple, so this
            // layout is valid. Calling `System` directly avoids recursive use
            // of Mnemosyne as the process global allocator while giving Miri a
            // provenance-tracked backing allocation.
            let layout = unsafe { Layout::from_size_align_unchecked(size, OS_PAGE_ALIGN) };
            return unsafe { System.alloc(layout) };
        }

        #[cfg(not(miri))]
        {
            // Safety: Raw system call to VirtualAlloc to commit and reserve virtual memory.
            // Size is validated at call sites to be non-zero and aligned.
            let ptr = unsafe {
                VirtualAlloc(
                    core::ptr::null(),
                    size,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                )
            };
            if ptr.is_null() {
                core::ptr::null_mut()
            } else {
                ptr as *mut u8
            }
        }
    }

    /// Releases virtual memory pages previously allocated with `allocate`.
    ///
    /// # Safety
    ///
    /// The `ptr` must be the exact base address returned by `allocate` and
    /// cannot be used after release.
    unsafe fn deallocate(ptr: *mut u8, _size: usize) -> bool {
        if ptr.is_null() {
            return false;
        }
        #[cfg(miri)]
        {
            const OS_PAGE_ALIGN: usize = 4096;
            // SAFETY: `ptr` came from the Miri `allocate` branch with this
            // exact size and alignment by the MemoryBackend contract.
            let layout = unsafe { Layout::from_size_align_unchecked(_size, OS_PAGE_ALIGN) };
            unsafe { System.dealloc(ptr, layout) };
            return true;
        }

        #[cfg(not(miri))]
        {
            // Safety: Raw system call to VirtualFree. The ptr must have been previously
            // returned by VirtualAlloc and not yet freed. MEM_RELEASE releases the whole region.
            let res = unsafe { VirtualFree(ptr as *mut c_void, 0, MEM_RELEASE) };
            debug_assert_ne!(res, 0, "VirtualFree failed");
            res != 0
        }
    }

    /// Asks the Memory Manager to discard the physical backing of a
    /// page range while keeping the mapping committed. Implemented as
    /// `VirtualAlloc(ptr, size, MEM_RESET, PAGE_READWRITE)`, which is
    /// the documented Win32 equivalent of `madvise(MADV_DONTNEED)`.
    /// `VirtualAlloc` with `MEM_RESET` returns the base address on
    /// success and `NULL` on failure, so we map the return into a
    /// boolean release status.
    unsafe fn page_reset(ptr: *mut u8, size: usize) -> bool {
        if ptr.is_null() || size == 0 {
            return false;
        }
        #[cfg(miri)]
        {
            return false;
        }
        #[cfg(not(miri))]
        {
            // Safety: ptr is inside an active VirtualAlloc-managed region and
            // size is a multiple of the system page size; MEM_RESET keeps the
            // mapping committed and never invalidates the address range.
            let result =
                unsafe { VirtualAlloc(ptr as *const c_void, size, MEM_RESET, PAGE_READWRITE) };
            !result.is_null()
        }
    }

    /// Releases the commit charge of a page range via
    /// `VirtualFree(MEM_DECOMMIT)` while keeping the address reservation, so a
    /// later `VirtualFree(MEM_RELEASE)` of the base still covers it. Returns
    /// `true` on success. Used to drop the eagerly-committed alignment slack of
    /// aligned segment/huge mappings.
    ///
    /// # Safety
    ///
    /// `ptr`/`size` must describe a page-aligned subrange of an active
    /// reservation that holds no live data; the range faults on access until
    /// re-committed or the base reservation is released.
    unsafe fn decommit(ptr: *mut u8, size: usize) -> bool {
        if ptr.is_null() || size == 0 {
            return false;
        }
        #[cfg(miri)]
        {
            return false;
        }
        #[cfg(not(miri))]
        {
            // Safety: ptr/size describe a page-aligned subrange of a live
            // VirtualAlloc reservation; MEM_DECOMMIT keeps the reservation valid.
            let res = unsafe { VirtualFree(ptr as *mut c_void, size, MEM_DECOMMIT) };
            res != 0
        }
    }

    /// Installs a `PAGE_NOACCESS` guard region via `VirtualProtect`.
    /// Subsequent reads or writes to the protected pages raise an
    /// access-violation. The mapping itself remains reserved, so a
    /// later `deallocate` covering the range still releases cleanly.
    unsafe fn make_guard(ptr: *mut u8, size: usize) -> bool {
        if ptr.is_null() || size == 0 {
            return false;
        }
        #[cfg(miri)]
        {
            return false;
        }
        #[cfg(not(miri))]
        {
            let mut old_protect: u32 = 0;
            // Safety: ptr is inside an active VirtualAlloc-managed region and
            // size is a multiple of the system page size. VirtualProtect
            // changes only the protection bits.
            let res = unsafe {
                VirtualProtect(
                    ptr as *mut c_void,
                    size,
                    PAGE_NOACCESS,
                    &mut old_protect as *mut u32,
                )
            };
            res != 0
        }
    }
}
