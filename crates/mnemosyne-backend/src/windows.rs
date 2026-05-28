//! Windows VirtualAlloc/VirtualFree memory backend.

use core::ffi::c_void;

// Windows API constants
const MEM_COMMIT: u32 = 0x00001000;
const MEM_RESERVE: u32 = 0x00002000;
const MEM_RELEASE: u32 = 0x00008000;
const PAGE_READWRITE: u32 = 0x04;

extern "system" {
    fn VirtualAlloc(
        lpAddress: *const c_void,
        dwSize: usize,
        flAllocationType: u32,
        flProtect: u32,
    ) -> *mut c_void;

    fn VirtualFree(lpAddress: *mut c_void, dwSize: usize, dwFreeType: u32) -> i32;
}

/// Windows virtual memory backend using `VirtualAlloc`/`VirtualFree`.
pub struct WindowsBackend;

impl mnemosyne_core::MemoryBackend for WindowsBackend {
    /// Reserves and commits virtual memory pages of the given size.
    ///
    /// # Safety
    ///
    /// The size must be a multiple of the system page size (usually 4KB).
    unsafe fn allocate(size: usize) -> *mut u8 {
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
        // Safety: Raw system call to VirtualFree. The ptr must have been previously
        // returned by VirtualAlloc and not yet freed. MEM_RELEASE releases the whole region.
        let res = unsafe { VirtualFree(ptr as *mut c_void, 0, MEM_RELEASE) };
        debug_assert_ne!(res, 0, "VirtualFree failed");
        res != 0
    }
}
