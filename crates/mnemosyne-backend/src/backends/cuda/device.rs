use core::ffi::c_void;
use core::sync::atomic::Ordering;

use mnemosyne_core::MemoryBackend;

use super::registry::CUDA_DEVICE_ALLOCATIONS;
use super::{
    CudaAllocOps, CudaAllocationRegistry, cuda_allocate, cuda_deallocate, loader,
    managed_raw_alloc, managed_raw_free,
};

/// A memory backend allocating CUDA device memory.
///
/// Under the hood, this uses CUDA unified memory (`cuMemAllocManaged`) and
/// advises the driver to prefer device placement (`cuMemAdvise` with
/// `CU_MEM_ADVISE_SET_PREFERRED_LOCATION`). This allows the host CPU to write
/// allocator metadata in-band without segfaulting, while keeping the
/// allocation device-preferred for optimal kernel performance.
///
/// `allocate` returns null on failure (driver unavailable, driver allocation
/// failure, or registry full); there is no host fallback.
pub struct CudaDeviceBackend;

impl CudaAllocOps for CudaDeviceBackend {
    #[inline]
    fn registry() -> &'static CudaAllocationRegistry {
        &CUDA_DEVICE_ALLOCATIONS
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

    #[inline]
    unsafe fn post_register(ptr: *mut u8, size: usize) {
        let advise_sym = loader::CU_MEM_ADVISE.load(Ordering::Acquire);
        if advise_sym.is_null() {
            return;
        }
        type CuMemAdviseFn = unsafe extern "system" fn(u64, usize, u32, i32) -> core::ffi::c_int;
        // SAFETY: transmute maps the verified dynamic library symbol address
        // to a function pointer with system calling convention.
        let cu_mem_advise: CuMemAdviseFn = unsafe { core::mem::transmute(advise_sym) };
        // CU_MEM_ADVISE_SET_PREFERRED_LOCATION = 3, device ordinal 0.
        // Placement advice is best-effort tuning: a nonzero status leaves the
        // allocation valid and host-accessible, so there is no failure to
        // surface or recover from here.
        // SAFETY: `ptr` is a live managed allocation of `size` bytes per the
        // trait contract.
        let _advise_status = unsafe { cu_mem_advise(ptr as u64, size, 3, 0) };
    }
}

impl MemoryBackend for CudaDeviceBackend {
    /// Allocates device-preferred CUDA managed memory. Returns null on
    /// failure (driver unavailable, driver allocation failure, or registry
    /// full).
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

/// Allocates through the shared device driver while allowing the arena to
/// keep a distinct pool identity for a device memory tier.
///
/// CUDA's managed allocation API does not expose an HBM-versus-GDDR selector;
/// the tier-specific wrappers therefore preserve the provider's device
/// allocation semantics and split allocator-local retention state. A future
/// provider with an explicit tier selector can replace the wrapper's
/// monomorphic forwarding without changing the heap dispatch surface.
#[inline(always)]
unsafe fn allocate_device_tier(size: usize) -> *mut u8 {
    // SAFETY: the caller upholds the `MemoryBackend::allocate` contract, and
    // `CudaDeviceBackend` is the shared driver-backed implementation.
    unsafe { CudaDeviceBackend::allocate(size) }
}

/// Releases a tier-keyed device allocation through the shared CUDA driver.
#[inline(always)]
unsafe fn deallocate_device_tier(ptr: *mut u8, size: usize) -> bool {
    // SAFETY: the caller upholds the `MemoryBackend::deallocate` contract, and
    // the pointer was allocated by the shared device backend.
    unsafe { CudaDeviceBackend::deallocate(ptr, size) }
}

/// A zero-sized device backend with an allocator-local HBM pool identity.
///
/// The current CUDA provider uses the same managed-memory driver operation as
/// [`CudaDeviceBackend`]. This type separates segment and thread-local pool
/// ownership for `MemoryTier::Hbm` without adding a runtime dispatch branch.
pub struct CudaHbmBackend;

/// A zero-sized device backend with an allocator-local GDDR pool identity.
///
/// The current CUDA provider uses the same managed-memory driver operation as
/// [`CudaDeviceBackend`]. This type separates segment and thread-local pool
/// ownership for `MemoryTier::Gddr` without adding a runtime dispatch branch.
pub struct CudaGddrBackend;

const _: () = assert!(
    core::mem::size_of::<CudaHbmBackend>() == 0
        && core::mem::size_of::<CudaGddrBackend>() == 0
        && core::mem::align_of::<CudaHbmBackend>() == 1
        && core::mem::align_of::<CudaGddrBackend>() == 1
);

impl_device_tier_backend!(CudaHbmBackend);
impl_device_tier_backend!(CudaGddrBackend);
