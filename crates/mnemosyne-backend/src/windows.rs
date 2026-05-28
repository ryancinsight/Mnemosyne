//! Windows VirtualAlloc/VirtualFree memory backend.

use core::ffi::c_void;

// Windows API constants
const MEM_COMMIT: u32 = 0x00001000;
const MEM_RESERVE: u32 = 0x00002000;
const MEM_RELEASE: u32 = 0x00008000;
/// `MEM_RESET` advises the Memory Manager that the addressed pages no
/// longer need to retain their contents; the OS may discard them and
/// re-zero on next access, but the mapping itself stays committed.
const MEM_RESET: u32 = 0x00080000;
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
        // Safety: ptr is inside an active VirtualAlloc-managed region and
        // size is a multiple of the system page size; MEM_RESET keeps the
        // mapping committed and never invalidates the address range.
        let result = unsafe { VirtualAlloc(ptr as *const c_void, size, MEM_RESET, PAGE_READWRITE) };
        !result.is_null()
    }
}
