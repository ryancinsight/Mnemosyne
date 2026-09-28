use core::ffi::c_void;
use core::sync::atomic::Ordering;

use super::registry::CUDA_ALLOCATIONS;
use super::{CudaAllocOps, CudaAllocationRegistry, loader, managed_raw_alloc, managed_raw_free};

/// A zero-copy memory backend mapping memory blocks directly using CUDA
/// managed memory.
///
/// `allocate` returns null when the NVIDIA driver is not loaded, when the
/// driver allocation fails, or when the bounded CUDA allocation registry is
/// full (the fresh allocation is released first). There is no host fallback;
/// callers must select another backend on null.
pub struct CudaUnifiedBackend;

impl CudaAllocOps for CudaUnifiedBackend {
    #[inline]
    fn registry() -> &'static CudaAllocationRegistry {
        &CUDA_ALLOCATIONS
    }

    #[inline]
    fn alloc_sym() -> *mut c_void {
        loader::CU_MEM_ALLOC_MANAGED.load(Ordering::Acquire)
    }

    #[inline]
    fn free_sym() -> *mut c_void {
        loader::CU_MEM_FREE.load(Ordering::Acquire)
    }

    #[inline]
    unsafe fn raw_alloc(alloc_sym: *mut c_void, size: usize) -> *mut u8 {
        // SAFETY: forwarded caller contract (resolved `cuMemAllocManaged`).
        unsafe { managed_raw_alloc(alloc_sym, size) }
    }

    #[inline]
    unsafe fn raw_free(free_sym: *mut c_void, ptr: *mut u8) -> core::ffi::c_int {
        // SAFETY: forwarded caller contract (resolved `cuMemFree`, live ptr).
        unsafe { managed_raw_free(free_sym, ptr) }
    }
}

impl_cuda_memory_backend!(CudaUnifiedBackend);
