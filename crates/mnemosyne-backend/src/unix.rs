//! Unix mmap/munmap memory backend.

use core::ffi::{c_int, c_void};
#[cfg(target_os = "linux")]
use mnemosyne_core::constants::SEGMENT_SIZE;

const PROT_READ: c_int = 1;
const PROT_WRITE: c_int = 2;
const MAP_PRIVATE: c_int = 2;

#[cfg(target_os = "macos")]
const MAP_ANON: c_int = 0x1000;
#[cfg(not(target_os = "macos"))]
const MAP_ANON: c_int = 0x20;

const MAP_FAILED: *mut c_void = -1isize as *mut c_void;

/// Linux `MADV_HUGEPAGE` advice constant.
///
/// Hints the kernel that the mapping is a good candidate for Transparent
/// Huge Pages (THP) promotion. On a 2 MiB-aligned, 2 MiB-multiple mapping
/// (matching `SEGMENT_SIZE` and `SEGMENT_ALIGN`) the kernel can typically
/// back the mapping with a single 2 MiB huge page, halving TLB pressure
/// for hot segment metadata access. Defined as `14` on Linux since 2.6.38.
#[cfg(target_os = "linux")]
const MADV_HUGEPAGE: c_int = 14;

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

    #[cfg(target_os = "linux")]
    fn madvise(addr: *mut c_void, length: usize, advice: c_int) -> c_int;
}

/// Issues a Linux `MADV_HUGEPAGE` hint for a freshly mapped segment-sized
/// region. The advice is purely advisory: kernels without THP support or
/// userspace-disabled-THP simply ignore it, so a failure return is dropped
/// silently and does not affect mapping validity.
///
/// On non-Linux Unix targets the hint is a no-op because the same advice
/// constant does not exist or has different semantics.
///
/// # Safety
///
/// `ptr` must be the base of a mapping of at least `length` bytes, and
/// `length` must be the exact mapped length.
#[inline]
unsafe fn hint_hugepage(ptr: *mut u8, length: usize) {
    #[cfg(target_os = "linux")]
    {
        if length >= SEGMENT_SIZE && length % SEGMENT_SIZE == 0 {
            // Safety: caller guarantees the mapping covers `length` bytes; madvise
            // is advisory and never invalidates the mapping on failure.
            let _ = unsafe { madvise(ptr as *mut c_void, length, MADV_HUGEPAGE) };
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Reference the arguments so the function signature stays stable
        // across Unix targets without a dead-argument warning.
        let _ = ptr;
        let _ = length;
    }
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
            return core::ptr::null_mut();
        }
        let ptr = ptr as *mut u8;
        // Safety: ptr is a valid mapping of `size` bytes. The hint is advisory
        // and may be ignored by the kernel without affecting the mapping.
        unsafe { hint_hugepage(ptr, size) };
        ptr
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use mnemosyne_core::MemoryBackend;

    #[test]
    fn segment_sized_allocation_survives_hugepage_hint() {
        // The MADV_HUGEPAGE hint is purely advisory: a Linux kernel that
        // ignores it must still produce a mapping that allocate/deallocate
        // can round-trip without error, and reads/writes against the
        // returned region must succeed. This regression-guards the hint
        // path against accidentally treating a benign EINVAL from the
        // advice as a fatal mapping failure.
        let size = SEGMENT_SIZE;
        // Safety: SEGMENT_SIZE is a non-zero power-of-two multiple of the
        // system page size, satisfying the allocate contract.
        let ptr = unsafe { UnixBackend::allocate(size) };
        assert!(!ptr.is_null(), "segment-sized mapping must succeed");

        // Touch the boundary bytes to confirm the entire region is mapped.
        // Safety: ptr covers [0, size) bytes per the allocate contract.
        unsafe {
            ptr.write_volatile(0xAA);
            ptr.add(size - 1).write_volatile(0x55);
            assert_eq!(ptr.read_volatile(), 0xAA);
            assert_eq!(ptr.add(size - 1).read_volatile(), 0x55);
        }

        // Safety: ptr is the exact base of the size-byte mapping.
        let released = unsafe { UnixBackend::deallocate(ptr, size) };
        assert!(
            released,
            "munmap reported failure for segment-sized mapping"
        );
    }

    #[test]
    fn sub_segment_allocation_skips_hugepage_hint() {
        // Mappings smaller than SEGMENT_SIZE must not receive the hint
        // (it would be unaligned to the THP boundary and produce noise in
        // kernel logs). This test confirms the path still allocates,
        // populates the boundary bytes, and releases cleanly.
        let size = PAGE_SIZE_FALLBACK;
        // Safety: size is a non-zero multiple of the system page size.
        let ptr = unsafe { UnixBackend::allocate(size) };
        assert!(!ptr.is_null());

        unsafe {
            ptr.write_volatile(0xAA);
            ptr.add(size - 1).write_volatile(0x55);
        }

        let released = unsafe { UnixBackend::deallocate(ptr, size) };
        assert!(released);
    }

    /// 4 KiB is the system page size on every Linux configuration this test
    /// runs against; explicit to avoid importing `mnemosyne_core::PAGE_SIZE`
    /// (which is the allocator-domain page size of 64 KiB, not the OS page).
    const PAGE_SIZE_FALLBACK: usize = 4096;
}
