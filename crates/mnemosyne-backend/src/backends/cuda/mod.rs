//! CUDA driver-backed memory backends with dynamic loading.
//!
//! `allocate` returns null when the NVIDIA driver is unavailable, when the
//! driver allocation call fails, or when the backend's bounded allocation
//! registry is full (the fresh driver allocation is released before
//! returning). There is no host-allocation fallback in these backends;
//! callers select an alternative backend explicitly.
//!
//! Module layout, one concern per leaf:
//!
//! - `loader`: driver library loading (cached handle), symbol resolution,
//!   and the one-time atomic initialization state machine.
//! - `registry`: bounded lock-free registries of live CUDA pointers.
//! - `context`: temporary CUDA context helpers for tests and probes.
//! - `veh` (Windows only): vectored-exception isolation of the `cuInit`
//!   probe.
//! - this module: the three `MemoryBackend` impls, expressed as thin
//!   zero-sized strategies over one generic allocate/deallocate driver
//!   (`CudaAllocOps`), monomorphized per backend with no dynamic dispatch.

mod context;
mod loader;
mod registry;
#[cfg(target_family = "windows")]
mod veh;

pub use context::{create_temp_context, destroy_temp_context};
pub use registry::CudaAllocationRegistry;

use core::ffi::c_void;
use core::sync::atomic::Ordering;
use mnemosyne_core::MemoryBackend;
use registry::{
    CUDA_ALLOCATIONS, CUDA_DEVICE_ALLOCATIONS, CUDA_HOST_PINNED_ALLOCATIONS, register_cuda_ptr_in,
    unregister_cuda_ptr_in,
};

/// Zero-sized strategy surface for the shared CUDA allocate/deallocate
/// driver.
///
/// Each backend supplies its registry, its resolved driver entry points, and
/// the raw allocation/release calls; [`cuda_allocate`] and
/// [`cuda_deallocate`] own the shared allocate → register → post-register /
/// rollback and unregister → free control flow once.
trait CudaAllocOps {
    /// Registry tracking this backend's live pointers.
    fn registry() -> &'static CudaAllocationRegistry;

    /// Resolved allocation entry point, or null while the driver is
    /// unavailable.
    fn alloc_sym() -> *mut c_void;

    /// Resolved release entry point, or null while the driver is unavailable.
    fn free_sym() -> *mut c_void;

    /// Calls the raw driver allocation entry point. Returns null on failure.
    ///
    /// # Safety
    ///
    /// `alloc_sym` must be this strategy's resolved, non-null allocation
    /// export; `size` must satisfy the `MemoryBackend::allocate` contract.
    unsafe fn raw_alloc(alloc_sym: *mut c_void, size: usize) -> *mut u8;

    /// Calls the raw driver release entry point and returns the driver
    /// status (0 = success).
    ///
    /// # Safety
    ///
    /// `free_sym` must be this strategy's resolved, non-null release export
    /// and `ptr` must be a live allocation produced by [`Self::raw_alloc`].
    unsafe fn raw_free(free_sym: *mut c_void, ptr: *mut u8) -> core::ffi::c_int;

    /// Hook invoked after successful registration (e.g. placement advice).
    /// The default does nothing.
    ///
    /// # Safety
    ///
    /// `ptr` must be a live allocation of `size` bytes produced by
    /// [`Self::raw_alloc`].
    #[inline]
    unsafe fn post_register(_ptr: *mut u8, _size: usize) {}
}

/// Shared allocate driver: initialize → resolve entry points → raw allocate
/// → register → post-register hook, rolling the raw allocation back when the
/// registry is full. Returns null on any failure.
///
/// # Safety
///
/// `size` must satisfy the `MemoryBackend::allocate` contract.
#[inline]
unsafe fn cuda_allocate<Ops: CudaAllocOps>(size: usize) -> *mut u8 {
    // SAFETY: `init_cuda` is safe for concurrent callers (atomic one-time
    // state machine).
    unsafe { loader::init_cuda() };
    let alloc_sym = Ops::alloc_sym();
    let free_sym = Ops::free_sym();
    if alloc_sym.is_null() || free_sym.is_null() {
        return core::ptr::null_mut();
    }

    // SAFETY: both entry points resolved non-null from the loaded driver;
    // the caller upholds the size contract.
    let ptr = unsafe { Ops::raw_alloc(alloc_sym, size) };
    if ptr.is_null() {
        return core::ptr::null_mut();
    }

    if register_cuda_ptr_in(Ops::registry(), ptr) {
        // SAFETY: `ptr` is the live allocation just produced by `raw_alloc`.
        unsafe { Ops::post_register(ptr, size) };
        return ptr;
    }

    // Registry full: this allocation cannot be tracked for release, so the
    // failure is surfaced as null after rolling the driver allocation back.
    // A nonzero rollback status leaves no recovery action here — the driver
    // owns the mapping and the allocation is already being reported failed.
    // SAFETY: `ptr` came from `raw_alloc` with the matching release export.
    let _rollback_status = unsafe { Ops::raw_free(free_sym, ptr) };
    core::ptr::null_mut()
}

/// Shared deallocate driver: unregister → raw free. Returns `false` when
/// `ptr` is not a tracked allocation of this backend or the driver reports a
/// release failure.
///
/// # Safety
///
/// `ptr` must satisfy the `MemoryBackend::deallocate` contract.
#[inline]
unsafe fn cuda_deallocate<Ops: CudaAllocOps>(ptr: *mut u8) -> bool {
    if !unregister_cuda_ptr_in(Ops::registry(), ptr) {
        return false;
    }
    let free_sym = Ops::free_sym();
    if free_sym.is_null() {
        return false;
    }
    // SAFETY: `ptr` was registered by `cuda_allocate`, so it is a live
    // allocation from this strategy's matching allocation export.
    unsafe { Ops::raw_free(free_sym, ptr) == 0 }
}

/// Raw `cuMemAllocManaged` call shared by the unified and device strategies.
///
/// # Safety
///
/// `alloc_sym` must be the resolved, non-null `cuMemAllocManaged` export.
unsafe fn managed_raw_alloc(alloc_sym: *mut c_void, size: usize) -> *mut u8 {
    type CuMemAllocManagedFn = unsafe extern "system" fn(*mut u64, usize, u32) -> core::ffi::c_int;
    // SAFETY: transmute maps the verified dynamic library symbol address to a
    // function pointer with system calling convention.
    let cu_mem_alloc_managed: CuMemAllocManagedFn = unsafe { core::mem::transmute(alloc_sym) };

    let mut dptr: u64 = 0;
    // CU_MEM_ATTACH_GLOBAL = 0x01
    // SAFETY: on a zero return, the driver wrote a device pointer valid for
    // `size` bytes into `dptr`.
    let res = unsafe { cu_mem_alloc_managed(&mut dptr, size, 0x01) };
    if res == 0 && dptr != 0 {
        dptr as *mut u8
    } else {
        core::ptr::null_mut()
    }
}

/// Raw `cuMemFree` call shared by the unified and device strategies.
///
/// # Safety
///
/// `free_sym` must be the resolved, non-null `cuMemFree` export and `ptr` a
/// live `cuMemAllocManaged` allocation.
unsafe fn managed_raw_free(free_sym: *mut c_void, ptr: *mut u8) -> core::ffi::c_int {
    type CuMemFreeFn = unsafe extern "system" fn(u64) -> core::ffi::c_int;
    // SAFETY: transmute maps the verified dynamic library symbol address to a
    // function pointer with system calling convention.
    let cu_mem_free: CuMemFreeFn = unsafe { core::mem::transmute(free_sym) };
    // SAFETY: `ptr` is a live managed allocation per the caller contract.
    unsafe { cu_mem_free(ptr as u64) }
}

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

impl MemoryBackend for CudaUnifiedBackend {
    /// Allocates CUDA unified managed memory. Returns null on failure
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

/// Returns true if the CUDA unified memory driver was successfully resolved.
pub fn is_cuda_available() -> bool {
    // SAFETY: `init_cuda` is safe to call concurrently (atomic one-time
    // state machine).
    unsafe { loader::init_cuda() };
    !loader::CU_MEM_ALLOC_MANAGED
        .load(Ordering::Acquire)
        .is_null()
}
