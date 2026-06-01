//! CUDA Unified Memory virtual allocation backend with dynamic loading and host fallback.

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU8, Ordering};
use mnemosyne_core::MemoryBackend;

#[cfg(target_family = "windows")]
extern "system" {
    fn LoadLibraryA(lpLibFileName: *const u8) -> *mut c_void;
    fn GetProcAddress(hModule: *mut c_void, lpProcName: *const u8) -> *mut c_void;
}

#[cfg(target_family = "unix")]
extern "C" {
    fn dlopen(filename: *const u8, flag: core::ffi::c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const u8) -> *mut c_void;
}

#[cfg(target_family = "unix")]
const RTLD_LAZY: core::ffi::c_int = 1;

static CU_INIT: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static CU_MEM_ALLOC_MANAGED: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static CU_MEM_FREE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static CUDA_INIT_STATE: AtomicU8 = AtomicU8::new(CUDA_UNINITIALIZED);

const CUDA_UNINITIALIZED: u8 = 0;
const CUDA_INITIALIZING: u8 = 1;
const CUDA_INITIALIZED: u8 = 2;

// The CUDA driver owns deallocation, so this backend tracks only live managed
// pointers that must route to cuMemFree. The fixed registry bounds metadata
// without heap allocation; overflow frees the CUDA allocation and falls back to
// the host backend.
const MAX_TRACKED_CUDA_ALLOCATIONS: usize = 256;
type CudaAllocationRegistry = [core::sync::atomic::AtomicPtr<u8>; MAX_TRACKED_CUDA_ALLOCATIONS];
static CUDA_ALLOCATIONS: CudaAllocationRegistry =
    [const { core::sync::atomic::AtomicPtr::new(core::ptr::null_mut()) };
        MAX_TRACKED_CUDA_ALLOCATIONS];

/// Exact count of active CUDA allocations inside the registry.
static CUDA_ALLOCATION_COUNT: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

fn register_cuda_ptr_in(registry: &CudaAllocationRegistry, ptr: *mut u8) -> bool {
    let start_idx = (ptr as usize >> 12) % MAX_TRACKED_CUDA_ALLOCATIONS;
    for i in 0..MAX_TRACKED_CUDA_ALLOCATIONS {
        let idx = (start_idx + i) % MAX_TRACKED_CUDA_ALLOCATIONS;
        let slot = &registry[idx];
        // Double-check: cheap relaxed load avoids CAS invalidations on populated slots.
        if slot.load(Ordering::Relaxed).is_null()
            && slot
                .compare_exchange(
                    core::ptr::null_mut(),
                    ptr,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
        {
            CUDA_ALLOCATION_COUNT.fetch_add(1, Ordering::Release);
            return true;
        }
    }
    false
}

fn unregister_cuda_ptr_in(registry: &CudaAllocationRegistry, ptr: *mut u8) -> bool {
    let active_count = CUDA_ALLOCATION_COUNT.load(Ordering::Acquire);
    if active_count == 0 {
        return false;
    }

    let mut seen_non_null = 0;
    let start_idx = (ptr as usize >> 12) % MAX_TRACKED_CUDA_ALLOCATIONS;
    for i in 0..MAX_TRACKED_CUDA_ALLOCATIONS {
        let idx = (start_idx + i) % MAX_TRACKED_CUDA_ALLOCATIONS;
        let slot = &registry[idx];
        let val = slot.load(Ordering::Relaxed);
        if !val.is_null() {
            seen_non_null += 1;
            if val == ptr
                && slot
                    .compare_exchange(
                        ptr,
                        core::ptr::null_mut(),
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
            {
                CUDA_ALLOCATION_COUNT.fetch_sub(1, Ordering::Release);
                return true;
            }
            if seen_non_null >= active_count {
                break;
            }
        }
    }
    false
}

fn is_cuda_ptr_in(registry: &CudaAllocationRegistry, ptr: *mut u8) -> bool {
    let active_count = CUDA_ALLOCATION_COUNT.load(Ordering::Acquire);
    if active_count == 0 {
        return false;
    }

    let mut seen_non_null = 0;
    let start_idx = (ptr as usize >> 12) % MAX_TRACKED_CUDA_ALLOCATIONS;
    for i in 0..MAX_TRACKED_CUDA_ALLOCATIONS {
        let idx = (start_idx + i) % MAX_TRACKED_CUDA_ALLOCATIONS;
        let val = registry[idx].load(Ordering::Relaxed);
        if !val.is_null() {
            seen_non_null += 1;
            if val == ptr {
                return true;
            }
            if seen_non_null >= active_count {
                break;
            }
        }
    }
    false
}

fn is_cuda_ptr(ptr: *mut u8) -> bool {
    is_cuda_ptr_in(&CUDA_ALLOCATIONS, ptr)
}

/// Initializes the CUDA Unified Memory driver by dynamically resolving driver symbols.
///
/// This function is safe to call concurrently from multiple threads because it uses
/// an atomic state machine (`CUDA_INIT_STATE`) to ensure `init_cuda_once` is executed
/// exactly once. Other calling threads will spin-wait until the initialization completes.
///
/// # Safety
///
/// Callers must guarantee that no allocator operations attempt to invoke CUDA backend calls
/// before or during this initialization phase without proper synchronization.
unsafe fn init_cuda() {
    if CUDA_INIT_STATE.load(Ordering::Acquire) == CUDA_INITIALIZED {
        return;
    }

    if CUDA_INIT_STATE
        .compare_exchange(
            CUDA_UNINITIALIZED,
            CUDA_INITIALIZING,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
    {
        // Safety: This thread owns the one-time CUDA symbol resolution phase.
        unsafe { init_cuda_once() };
        CUDA_INIT_STATE.store(CUDA_INITIALIZED, Ordering::Release);
        return;
    }

    while CUDA_INIT_STATE.load(Ordering::Acquire) != CUDA_INITIALIZED {
        core::hint::spin_loop();
    }
}

fn register_cuda_ptr(ptr: *mut u8) -> bool {
    register_cuda_ptr_in(&CUDA_ALLOCATIONS, ptr)
}

fn unregister_cuda_ptr(ptr: *mut u8) -> bool {
    unregister_cuda_ptr_in(&CUDA_ALLOCATIONS, ptr)
}

/// Resolves CUDA driver API symbols dynamically from the system's driver libraries.
///
/// # Safety
///
/// This function is unsafe because it performs OS-specific dynamic library loading and
/// raw pointer resolution. The caller must guarantee:
/// - Exclusive access during symbol loading (enforced by the parent `init_cuda` caller).
/// - The resolved symbols must be valid function pointers and must not be mutated.
unsafe fn init_cuda_once() {
    let lib = {
        #[cfg(target_family = "windows")]
        {
            LoadLibraryA(c"nvcuda.dll".as_ptr() as *const u8)
        }
        #[cfg(target_family = "unix")]
        {
            let p = dlopen(c"libcuda.so".as_ptr() as *const u8, RTLD_LAZY);
            if p.is_null() {
                dlopen(c"libcuda.so.1".as_ptr() as *const u8, RTLD_LAZY)
            } else {
                p
            }
        }
        #[cfg(not(any(target_family = "windows", target_family = "unix")))]
        {
            core::ptr::null_mut()
        }
    };

    if lib.is_null() {
        return;
    }

    let init_sym = {
        #[cfg(target_family = "windows")]
        {
            GetProcAddress(lib, c"cuInit".as_ptr() as *const u8)
        }
        #[cfg(target_family = "unix")]
        {
            dlsym(lib, c"cuInit".as_ptr() as *const u8)
        }
    };

    let alloc_sym = {
        #[cfg(target_family = "windows")]
        {
            GetProcAddress(lib, c"cuMemAllocManaged".as_ptr() as *const u8)
        }
        #[cfg(target_family = "unix")]
        {
            dlsym(lib, c"cuMemAllocManaged".as_ptr() as *const u8)
        }
    };

    let free_sym = {
        #[cfg(target_family = "windows")]
        {
            GetProcAddress(lib, c"cuMemFree".as_ptr() as *const u8)
        }
        #[cfg(target_family = "unix")]
        {
            dlsym(lib, c"cuMemFree".as_ptr() as *const u8)
        }
    };

    if !init_sym.is_null() && !alloc_sym.is_null() && !free_sym.is_null() {
        CU_INIT.store(init_sym, Ordering::Release);
        CU_MEM_ALLOC_MANAGED.store(alloc_sym, Ordering::Release);
        CU_MEM_FREE.store(free_sym, Ordering::Release);

        // Safety: transmute maps the verified dynamic library symbol address
        // to a function pointer with system calling convention.
        type CuInitFn = unsafe extern "system" fn(u32) -> core::ffi::c_int;
        let cu_init: CuInitFn = unsafe { core::mem::transmute::<*mut c_void, CuInitFn>(init_sym) };
        // Safety: cuInit is initialized with flags = 0 as specified by NVIDIA CUDA Driver API.
        let _ = unsafe { cu_init(0) };
    }
}

/// A zero-copy memory backend mapping memory blocks directly using CUDA managed memory.
///
/// Falls back to standard host OS allocation if the Nvidia driver is not loaded or if the
/// Bounded CUDA allocation registry is full.
pub struct CudaUnifiedBackend;

impl MemoryBackend for CudaUnifiedBackend {
    const SUPPORTS_PAGE_RESET: bool = <crate::DefaultBackend as MemoryBackend>::SUPPORTS_PAGE_RESET;
    const SUPPORTS_MAKE_GUARD: bool = <crate::DefaultBackend as MemoryBackend>::SUPPORTS_MAKE_GUARD;
    const SUPPORTS_DECOMMIT: bool = <crate::DefaultBackend as MemoryBackend>::SUPPORTS_DECOMMIT;

    /// Allocates CUDA unified managed memory, falling back to default host virtual memory on failure.
    ///
    /// # Safety
    ///
    /// The size must be greater than zero and page-aligned.
    #[inline]
    unsafe fn allocate(size: usize) -> *mut u8 {
        // Safety: Calls OS-specific library loading to initialize CUDA function pointers.
        unsafe { init_cuda() };
        let alloc_ptr = CU_MEM_ALLOC_MANAGED.load(Ordering::Acquire);
        let free_ptr = CU_MEM_FREE.load(Ordering::Acquire);
        if !alloc_ptr.is_null() && !free_ptr.is_null() {
            // Safety: transmute maps the verified dynamic library symbol address
            // to a function pointer with system calling convention.
            type CuMemAllocManagedFn =
                unsafe extern "system" fn(*mut u64, usize, u32) -> core::ffi::c_int;
            let cu_mem_alloc_managed: CuMemAllocManagedFn =
                unsafe { core::mem::transmute::<*mut c_void, CuMemAllocManagedFn>(alloc_ptr) };

            let mut dptr: u64 = 0;
            // CU_MEM_ATTACH_GLOBAL = 0x01
            // Safety: Dynamic call to cuMemAllocManaged allocated size bytes. dptr is a valid pointer if return value is 0.
            let res = unsafe { cu_mem_alloc_managed(&mut dptr, size, 0x01) };
            if res == 0 && dptr != 0 {
                let ptr = dptr as *mut u8;
                if register_cuda_ptr(ptr) {
                    return ptr;
                }
                // If registry is full, deallocate and fallback.
                // Safety: transmute maps the verified dynamic library symbol address
                // to a function pointer with system calling convention.
                type CuMemFreeFn = unsafe extern "system" fn(u64) -> core::ffi::c_int;
                let cu_mem_free: CuMemFreeFn =
                    unsafe { core::mem::transmute::<*mut c_void, CuMemFreeFn>(free_ptr) };
                // Safety: Releases the allocated managed memory because registration failed.
                let _ = unsafe { cu_mem_free(dptr) };
            }
        }

        // Safety: Fallback to default CPU OS virtual allocator.
        unsafe { <crate::DefaultBackend as MemoryBackend>::allocate(size) }
    }

    /// Deallocates memory allocated by this backend.
    ///
    /// # Safety
    ///
    /// The ptr must be valid and size must match the allocated size.
    #[inline]
    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        if unregister_cuda_ptr(ptr) {
            let free_ptr = CU_MEM_FREE.load(Ordering::Acquire);
            if !free_ptr.is_null() {
                // Safety: transmute maps the verified dynamic library symbol address
                // to a function pointer with system calling convention.
                type CuMemFreeFn = unsafe extern "system" fn(u64) -> core::ffi::c_int;
                let cu_mem_free: CuMemFreeFn =
                    unsafe { core::mem::transmute::<*mut c_void, CuMemFreeFn>(free_ptr) };
                // Safety: Dynamic call to cuMemFree releases the verified CUDA-allocated memory pointer.
                let status = unsafe { cu_mem_free(ptr as u64) };
                return status == 0;
            }
        }

        // Safety: Fallback to default CPU OS virtual deallocator.
        unsafe { <crate::DefaultBackend as MemoryBackend>::deallocate(ptr, size) }
    }

    /// Drops the physical backing of an idle page range while keeping the
    /// virtual mapping committed. For fallback host allocations, it forwards the
    /// reset call to `DefaultBackend`. For active CUDA managed memory, it returns `false`.
    ///
    /// # Safety
    ///
    /// Same contract as the wrapped `MemoryBackend::page_reset`.
    #[inline]
    unsafe fn page_reset(ptr: *mut u8, size: usize) -> bool {
        if !is_cuda_ptr(ptr) {
            // Safety: Forward to host OS allocator for host fallback mappings.
            unsafe { <crate::DefaultBackend as MemoryBackend>::page_reset(ptr, size) }
        } else {
            false
        }
    }

    /// Installs a guard region. For fallback host allocations, it forwards the
    /// guard install to `DefaultBackend`. For active CUDA managed memory, it returns `false`.
    ///
    /// # Safety
    ///
    /// Same contract as the wrapped `MemoryBackend::make_guard`.
    #[inline]
    unsafe fn make_guard(ptr: *mut u8, size: usize) -> bool {
        if !is_cuda_ptr(ptr) {
            // Safety: Forward to host OS allocator for host fallback mappings.
            unsafe { <crate::DefaultBackend as MemoryBackend>::make_guard(ptr, size) }
        } else {
            false
        }
    }

    /// Releases the commit charge of a page range. For fallback host allocations, it
    /// forwards the decommit to `DefaultBackend`. For active CUDA managed memory, it returns `false`.
    ///
    /// # Safety
    ///
    /// Same contract as the wrapped `MemoryBackend::decommit`.
    #[inline]
    unsafe fn decommit(ptr: *mut u8, size: usize) -> bool {
        if !is_cuda_ptr(ptr) {
            // Safety: Forward to host OS allocator for host fallback mappings.
            unsafe { <crate::DefaultBackend as MemoryBackend>::decommit(ptr, size) }
        } else {
            false
        }
    }
}

/// Returns true if the CUDA unified memory driver was successfully resolved.
pub fn is_cuda_available() -> bool {
    // Safety: Calls init_cuda which is safe to call concurrently.
    unsafe { init_cuda() };
    !CU_MEM_ALLOC_MANAGED.load(Ordering::Acquire).is_null()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_registry() -> CudaAllocationRegistry {
        [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_TRACKED_CUDA_ALLOCATIONS]
    }

    #[test]
    fn cuda_registry_is_bounded_and_reusable() {
        let registry = test_registry();
        let mut bytes = [0_u8; MAX_TRACKED_CUDA_ALLOCATIONS + 1];

        for byte in bytes.iter_mut().take(MAX_TRACKED_CUDA_ALLOCATIONS) {
            assert!(register_cuda_ptr_in(&registry, byte as *mut u8));
        }

        assert!(!register_cuda_ptr_in(
            &registry,
            &mut bytes[MAX_TRACKED_CUDA_ALLOCATIONS] as *mut u8
        ));
        assert!(unregister_cuda_ptr_in(&registry, &mut bytes[7] as *mut u8));
        assert!(register_cuda_ptr_in(
            &registry,
            &mut bytes[MAX_TRACKED_CUDA_ALLOCATIONS] as *mut u8
        ));
    }

    #[test]
    fn cuda_registry_rejects_unknown_pointers() {
        let registry = test_registry();
        let mut byte = 0_u8;

        assert!(!unregister_cuda_ptr_in(&registry, &mut byte as *mut u8));
    }

    #[test]
    fn cuda_registry_hashing_and_fallback_forwarding() {
        let registry = test_registry();
        let mut byte1 = 0_u8;
        let mut byte2 = 0_u8;

        let ptr1 = &mut byte1 as *mut u8;
        let ptr2 = &mut byte2 as *mut u8;

        assert!(register_cuda_ptr_in(&registry, ptr1));
        assert!(is_cuda_ptr_in(&registry, ptr1));
        assert!(!is_cuda_ptr_in(&registry, ptr2));

        assert!(unregister_cuda_ptr_in(&registry, ptr1));
        assert!(!is_cuda_ptr_in(&registry, ptr1));
    }
}
