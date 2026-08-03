use core::ffi::c_void;
use core::sync::atomic::Ordering;

use mnemosyne_core::MemoryBackend;

use super::registry::CUDA_HOST_PINNED_ALLOCATIONS;
use super::{CudaAllocOps, CudaAllocationRegistry, cuda_allocate, cuda_deallocate, loader};

/// A memory backend allocating CUDA page-locked (pinned) host memory.
///
/// `allocate` returns null on failure (driver unavailable, driver allocation
/// failure, or registry full); there is no host fallback.
pub struct CudaHostPinnedBackend;

impl CudaAllocOps for CudaHostPinnedBackend {
    #[inline]
    fn registry() -> &'static CudaAllocationRegistry {
        &CUDA_HOST_PINNED_ALLOCATIONS
    }

    #[inline]
    fn alloc_sym() -> *mut c_void {
        loader::CU_MEM_HOST_ALLOC.load(Ordering::Acquire)
    }

    #[inline]
    fn free_sym() -> *mut c_void {
        loader::CU_MEM_FREE_HOST.load(Ordering::Acquire)
    }

    #[inline]
    unsafe fn raw_alloc(alloc_sym: *mut c_void, size: usize) -> *mut u8 {
        type CuMemHostAllocFn =
            unsafe extern "system" fn(*mut *mut c_void, usize, u32) -> core::ffi::c_int;
        // SAFETY: transmute maps the verified dynamic library symbol address
        // to a function pointer with system calling convention.
        let cu_mem_host_alloc: CuMemHostAllocFn = unsafe { core::mem::transmute(alloc_sym) };

        let mut host_ptr: *mut c_void = core::ptr::null_mut();
        // CU_MEMHOSTALLOC_DEVICEMAP = 0x02
        // SAFETY: on a zero return, the driver wrote a host pointer valid for
        // `size` bytes into `host_ptr`.
        let res = unsafe { cu_mem_host_alloc(core::ptr::addr_of_mut!(host_ptr), size, 0x02) };
        if res == 0 && !host_ptr.is_null() {
            host_ptr as *mut u8
        } else {
            core::ptr::null_mut()
        }
    }

    #[inline]
    unsafe fn raw_free(free_sym: *mut c_void, ptr: *mut u8) -> core::ffi::c_int {
        type CuMemFreeHostFn = unsafe extern "system" fn(*mut c_void) -> core::ffi::c_int;
        // SAFETY: transmute maps the verified dynamic library symbol address
        // to a function pointer with system calling convention.
        let cu_mem_free_host: CuMemFreeHostFn = unsafe { core::mem::transmute(free_sym) };
        // SAFETY: `ptr` is a live `cuMemHostAlloc` allocation per the caller
        // contract.
        unsafe { cu_mem_free_host(ptr as *mut c_void) }
    }
}

impl MemoryBackend for CudaHostPinnedBackend {
    /// Allocates CUDA page-locked host memory. Returns null on failure
    /// (driver unavailable, driver allocation failure, or registry full).
    ///
    /// # Safety
    ///
    /// The size must be greater than zero and page-aligned.
    #[inline]
    unsafe fn allocate(size: usize) -> *mut u8 {
        // SAFETY: forwarded caller contract.
        unsafe { cuda_allocate::<Self>(size) }
    }

    /// Deallocates memory allocated by this backend.
    ///
    /// # Safety
    ///
    /// The ptr must be valid and size must match the allocated size.
    #[inline]
    unsafe fn deallocate(ptr: *mut u8, _size: usize) -> bool {
        // SAFETY: forwarded caller contract.
        unsafe { cuda_deallocate::<Self>(ptr) }
    }
}
