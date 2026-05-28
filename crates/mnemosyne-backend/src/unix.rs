//! Unix mmap/munmap memory backend.

use core::ffi::{c_int, c_void};

const PROT_READ: c_int = 1;
const PROT_WRITE: c_int = 2;
const MAP_PRIVATE: c_int = 2;

#[cfg(target_os = "macos")]
const MAP_ANON: c_int = 0x1000;
#[cfg(not(target_os = "macos"))]
const MAP_ANON: c_int = 0x20;

const MAP_FAILED: *mut c_void = -1isize as *mut c_void;

extern "C" {
    fn mmap(
        addr: *mut c_void,
        length: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: isize,
    ) -> *mut c_void;

    fn munmap(addr: *mut c_void, length: usize) -> c_int;
}

/// Unix virtual memory backend using `mmap`/`munmap`.
pub struct UnixBackend;

impl mnemosyne_core::MemoryBackend for UnixBackend {
    /// Allocates virtual memory pages of the given size.
    ///
    /// # Safety
    ///
    /// The size must be a multiple of the system page size (usually 4KB).
    unsafe fn allocate(size: usize) -> *mut u8 {
        // Safety: Raw system call to mmap to establish a private anonymous page mapping.
        // Size must be page-aligned and non-zero.
        let ptr = unsafe {
            mmap(
                core::ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANON,
                -1,
                0,
            )
        };
        if ptr == MAP_FAILED {
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
    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        if ptr.is_null() {
            return false;
        }
        // Safety: Raw system call to munmap. The ptr must point to a valid mapped region
        // of the specified size.
        let res = unsafe { munmap(ptr as *mut c_void, size) };
        debug_assert_eq!(res, 0, "munmap failed");
        res == 0
    }
}
