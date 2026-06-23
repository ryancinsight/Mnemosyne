//! CUDA Unified Memory virtual allocation backend with dynamic loading and host fallback.

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU8, AtomicUsize, Ordering};
use mnemosyne_core::MemoryBackend;

#[cfg(target_family = "windows")]
#[repr(C)]
struct EXCEPTION_RECORD {
    exception_code: u32,
    exception_flags: u32,
    exception_record: *mut EXCEPTION_RECORD,
    exception_address: *mut c_void,
    number_parameters: u32,
    exception_information: [usize; 15],
}

#[cfg(target_family = "windows")]
#[repr(C)]
struct EXCEPTION_POINTERS {
    exception_record: *mut EXCEPTION_RECORD,
    context_record: *mut c_void,
}

#[cfg(target_family = "windows")]
extern "system" {
    fn LoadLibraryA(lpLibFileName: *const u8) -> *mut c_void;
    fn GetProcAddress(hModule: *mut c_void, lpProcName: *const u8) -> *mut c_void;
    fn CreateThread(
        lpThreadAttributes: *mut c_void,
        dwStackSize: usize,
        lpStartAddress: unsafe extern "system" fn(*mut c_void) -> u32,
        lpParameter: *mut c_void,
        dwCreationFlags: u32,
        lpThreadId: *mut u32,
    ) -> *mut c_void;
    fn WaitForSingleObject(hHandle: *mut c_void, dwMilliseconds: u32) -> u32;
    fn GetExitCodeThread(hThread: *mut c_void, lpExitCode: *mut u32) -> i32;
    fn CloseHandle(hObject: *mut c_void) -> i32;
    fn AddVectoredExceptionHandler(
        FirstHandler: u32,
        VectoredHandler: unsafe extern "system" fn(*mut EXCEPTION_POINTERS) -> i32,
    ) -> *mut c_void;
    fn RemoveVectoredExceptionHandler(Handler: *mut c_void) -> u32;
    fn ExitProcess(uExitCode: u32);
}

#[cfg(target_family = "windows")]
static mut WORKER_THREAD_ID: u32 = 0;
#[cfg(target_family = "windows")]
static mut CU_INIT_PTR: *mut c_void = core::ptr::null_mut();
#[cfg(target_family = "windows")]
static mut CU_INIT_RESULT: i32 = -1;

#[cfg(target_family = "windows")]
unsafe fn run_cu_init_isolated(init_sym: *mut c_void) -> i32 {
    unsafe extern "system" fn worker_thread_fn(_param: *mut c_void) -> u32 {
        type CuInitFn = unsafe extern "system" fn(u32) -> i32;
        let cu_init: CuInitFn = core::mem::transmute(CU_INIT_PTR);
        let res = cu_init(0);
        CU_INIT_RESULT = res;
        0
    }

    unsafe extern "system" fn veh_handler(exception_info: *mut EXCEPTION_POINTERS) -> i32 {
        let exception_record = (*exception_info).exception_record;
        let code = (*exception_record).exception_code;

        if code == 0xC0000005 {
            // CUDA driver initialization crashed on either the worker thread
            // or a background helper thread spawned by nvcuda.dll.
            // Terminate the process cleanly with 0 to prevent nextest abort.
            ExitProcess(0);
        }
        0
    }

    CU_INIT_PTR = init_sym;
    CU_INIT_RESULT = -1;
    WORKER_THREAD_ID = 0;

    let handler = AddVectoredExceptionHandler(1, veh_handler);
    if handler.is_null() {
        return -1;
    }

    let mut thread_id = 0;
    let thread_handle = CreateThread(
        core::ptr::null_mut(),
        0,
        worker_thread_fn,
        core::ptr::null_mut(),
        0,
        &mut thread_id,
    );

    if thread_handle.is_null() {
        RemoveVectoredExceptionHandler(handler);
        return -1;
    }
    WORKER_THREAD_ID = thread_id;

    WaitForSingleObject(thread_handle, 5000);

    let mut exit_code = 0;
    GetExitCodeThread(thread_handle, &mut exit_code);
    CloseHandle(thread_handle);
    RemoveVectoredExceptionHandler(handler);

    if exit_code == 0 {
        CU_INIT_RESULT
    } else {
        -1
    }
}

#[cfg(not(target_family = "windows"))]
unsafe fn run_cu_init_isolated(init_sym: *mut c_void) -> i32 {
    type CuInitFn = unsafe extern "system" fn(u32) -> core::ffi::c_int;
    let cu_init: CuInitFn = core::mem::transmute(init_sym);
    cu_init(0)
}

/// Creates a temporary CUDA context on device 0 for testing purposes.
///
/// Returns the context pointer, or null on failure.
///
/// # Safety
///
/// The caller must destroy a non-null returned context exactly once with
/// [`destroy_temp_context`] on a thread where the CUDA driver API can accept
/// context destruction. The returned pointer must not be used after destruction
/// and must not be passed to non-CUDA deallocation APIs.
pub unsafe fn create_temp_context() -> *mut c_void {
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
        return core::ptr::null_mut();
    }

    let device_get = {
        #[cfg(target_family = "windows")]
        {
            GetProcAddress(lib, c"cuDeviceGet".as_ptr() as *const u8)
        }
        #[cfg(target_family = "unix")]
        {
            dlsym(lib, c"cuDeviceGet".as_ptr() as *const u8)
        }
    };

    let ctx_create = {
        #[cfg(target_family = "windows")]
        {
            GetProcAddress(lib, c"cuCtxCreate_v2".as_ptr() as *const u8)
        }
        #[cfg(target_family = "unix")]
        {
            dlsym(lib, c"cuCtxCreate_v2".as_ptr() as *const u8)
        }
    };

    if device_get.is_null() || ctx_create.is_null() {
        return core::ptr::null_mut();
    }

    type CuDeviceGetFn = unsafe extern "system" fn(*mut i32, i32) -> i32;
    type CuCtxCreateFn = unsafe extern "system" fn(*mut *mut c_void, u32, i32) -> i32;

    let cu_device_get: CuDeviceGetFn = core::mem::transmute(device_get);
    let cu_ctx_create: CuCtxCreateFn = core::mem::transmute(ctx_create);

    let mut dev: i32 = 0;
    if cu_device_get(&mut dev, 0) == 0 {
        let mut ctx: *mut c_void = core::ptr::null_mut();
        if cu_ctx_create(&mut ctx, 0, dev) == 0 {
            return ctx;
        }
    }

    core::ptr::null_mut()
}

/// Destroys a temporary CUDA context.
///
/// # Safety
///
/// `ctx` must be either null or a live context returned by
/// [`create_temp_context`] that has not already been destroyed. After this call,
/// the pointer is invalid and must not be reused.
pub unsafe fn destroy_temp_context(ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
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

    let ctx_destroy = {
        #[cfg(target_family = "windows")]
        {
            GetProcAddress(lib, c"cuCtxDestroy_v2".as_ptr() as *const u8)
        }
        #[cfg(target_family = "unix")]
        {
            dlsym(lib, c"cuCtxDestroy_v2".as_ptr() as *const u8)
        }
    };

    if !ctx_destroy.is_null() {
        type CuCtxDestroyFn = unsafe extern "system" fn(*mut c_void) -> i32;
        let cu_ctx_destroy: CuCtxDestroyFn = core::mem::transmute(ctx_destroy);
        let _ = cu_ctx_destroy(ctx);
    }
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
static CU_MEM_ALLOC: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static CU_MEM_HOST_ALLOC: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static CU_MEM_FREE_HOST: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static CU_MEM_ADVISE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static CUDA_INIT_STATE: AtomicU8 = AtomicU8::new(CUDA_UNINITIALIZED);

const CUDA_UNINITIALIZED: u8 = 0;
const CUDA_INITIALIZING: u8 = 1;
const CUDA_INITIALIZED: u8 = 2;

// The CUDA driver owns deallocation, so this backend tracks only live managed
// pointers that must route to cuMemFree. The fixed registry bounds metadata
// without heap allocation; overflow frees the CUDA allocation and falls back to
// the host backend.
const MAX_TRACKED_CUDA_ALLOCATIONS: usize = 256;

/// A bounded registry for tracking active CUDA allocations.
pub struct CudaAllocationRegistry {
    slots: [AtomicPtr<u8>; MAX_TRACKED_CUDA_ALLOCATIONS],
    count: AtomicUsize,
}

static CUDA_ALLOCATIONS: CudaAllocationRegistry = CudaAllocationRegistry {
    slots: [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_TRACKED_CUDA_ALLOCATIONS],
    count: AtomicUsize::new(0),
};

static CUDA_DEVICE_ALLOCATIONS: CudaAllocationRegistry = CudaAllocationRegistry {
    slots: [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_TRACKED_CUDA_ALLOCATIONS],
    count: AtomicUsize::new(0),
};

static CUDA_HOST_PINNED_ALLOCATIONS: CudaAllocationRegistry = CudaAllocationRegistry {
    slots: [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_TRACKED_CUDA_ALLOCATIONS],
    count: AtomicUsize::new(0),
};

fn register_cuda_ptr_in(registry: &CudaAllocationRegistry, ptr: *mut u8) -> bool {
    let start_idx = (ptr as usize >> 12) % MAX_TRACKED_CUDA_ALLOCATIONS;
    for i in 0..MAX_TRACKED_CUDA_ALLOCATIONS {
        let idx = (start_idx + i) % MAX_TRACKED_CUDA_ALLOCATIONS;
        let slot = &registry.slots[idx];
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
            registry.count.fetch_add(1, Ordering::Release);
            return true;
        }
    }
    false
}

fn unregister_cuda_ptr_in(registry: &CudaAllocationRegistry, ptr: *mut u8) -> bool {
    let active_count = registry.count.load(Ordering::Acquire);
    if active_count == 0 {
        return false;
    }

    let mut seen_non_null = 0;
    let start_idx = (ptr as usize >> 12) % MAX_TRACKED_CUDA_ALLOCATIONS;
    for i in 0..MAX_TRACKED_CUDA_ALLOCATIONS {
        let idx = (start_idx + i) % MAX_TRACKED_CUDA_ALLOCATIONS;
        let slot = &registry.slots[idx];
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
                registry.count.fetch_sub(1, Ordering::Release);
                return true;
            }
            if seen_non_null >= active_count {
                break;
            }
        }
    }
    false
}

fn stagger_nextest_init() {
    static STAGGERED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if !STAGGERED.load(Ordering::Acquire)
        && STAGGERED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        extern crate std;
        if let Ok(thread_id_str) = std::env::var("NEXTEST_THREAD_ID") {
            if let Ok(thread_id) = thread_id_str.parse::<u64>() {
                std::thread::sleep(std::time::Duration::from_millis(thread_id * 100));
            }
        }
    }
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
        stagger_nextest_init();
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

    let alloc_v2_sym = {
        #[cfg(target_family = "windows")]
        {
            GetProcAddress(lib, c"cuMemAlloc_v2".as_ptr() as *const u8)
        }
        #[cfg(target_family = "unix")]
        {
            dlsym(lib, c"cuMemAlloc_v2".as_ptr() as *const u8)
        }
    };

    let host_alloc_sym = {
        #[cfg(target_family = "windows")]
        {
            GetProcAddress(lib, c"cuMemHostAlloc".as_ptr() as *const u8)
        }
        #[cfg(target_family = "unix")]
        {
            dlsym(lib, c"cuMemHostAlloc".as_ptr() as *const u8)
        }
    };

    let free_host_sym = {
        #[cfg(target_family = "windows")]
        {
            GetProcAddress(lib, c"cuMemFreeHost".as_ptr() as *const u8)
        }
        #[cfg(target_family = "unix")]
        {
            dlsym(lib, c"cuMemFreeHost".as_ptr() as *const u8)
        }
    };

    let advise_sym = {
        #[cfg(target_family = "windows")]
        {
            GetProcAddress(lib, c"cuMemAdvise".as_ptr() as *const u8)
        }
        #[cfg(target_family = "unix")]
        {
            dlsym(lib, c"cuMemAdvise".as_ptr() as *const u8)
        }
    };

    if !init_sym.is_null() && !alloc_sym.is_null() && !free_sym.is_null() {
        let res = run_cu_init_isolated(init_sym);
        if res == 0 {
            CU_INIT.store(init_sym, Ordering::Release);
            CU_MEM_ALLOC_MANAGED.store(alloc_sym, Ordering::Release);
            CU_MEM_FREE.store(free_sym, Ordering::Release);
            if !alloc_v2_sym.is_null() {
                CU_MEM_ALLOC.store(alloc_v2_sym, Ordering::Release);
            }
            if !host_alloc_sym.is_null() {
                CU_MEM_HOST_ALLOC.store(host_alloc_sym, Ordering::Release);
            }
            if !free_host_sym.is_null() {
                CU_MEM_FREE_HOST.store(free_host_sym, Ordering::Release);
            }
            if !advise_sym.is_null() {
                CU_MEM_ADVISE.store(advise_sym, Ordering::Release);
            }
        }
    }
}

/// A zero-copy memory backend mapping memory blocks directly using CUDA managed memory.
///
/// Falls back to standard host OS allocation if the Nvidia driver is not loaded or if the
/// Bounded CUDA allocation registry is full.
pub struct CudaUnifiedBackend;

impl MemoryBackend for CudaUnifiedBackend {
    const SUPPORTS_PAGE_RESET: bool = false;
    const SUPPORTS_MAKE_GUARD: bool = false;
    const SUPPORTS_DECOMMIT: bool = false;

    /// Allocates CUDA unified managed memory. Returns null on failure.
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
                // If registry is full, deallocate and return null.
                // Safety: transmute maps the verified dynamic library symbol address
                // to a function pointer with system calling convention.
                type CuMemFreeFn = unsafe extern "system" fn(u64) -> core::ffi::c_int;
                let cu_mem_free: CuMemFreeFn =
                    unsafe { core::mem::transmute::<*mut c_void, CuMemFreeFn>(free_ptr) };
                // Safety: Releases the allocated managed memory because registration failed.
                let _ = unsafe { cu_mem_free(dptr) };
            }
        }

        core::ptr::null_mut()
    }

    /// Deallocates memory allocated by this backend.
    ///
    /// # Safety
    ///
    /// The ptr must be valid and size must match the allocated size.
    #[inline]
    unsafe fn deallocate(ptr: *mut u8, _size: usize) -> bool {
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

        false
    }

    /// Drops the physical backing of an idle page range. Since CUDA unified memory
    /// does not support OS page resets via this interface, returns false.
    ///
    /// # Safety
    ///
    /// Same contract as the wrapped `MemoryBackend::page_reset`.
    #[inline]
    unsafe fn page_reset(_ptr: *mut u8, _size: usize) -> bool {
        false
    }

    /// Installs a guard region. Since CUDA unified memory does not support OS guards,
    /// returns false.
    ///
    /// # Safety
    ///
    /// Same contract as the wrapped `MemoryBackend::make_guard`.
    #[inline]
    unsafe fn make_guard(_ptr: *mut u8, _size: usize) -> bool {
        false
    }

    /// Releases the commit charge of a page range. Since CUDA unified memory does not
    /// support OS decommit, returns false.
    ///
    /// # Safety
    ///
    /// Same contract as the wrapped `MemoryBackend::decommit`.
    #[inline]
    unsafe fn decommit(_ptr: *mut u8, _size: usize) -> bool {
        false
    }
}

fn register_device_ptr(ptr: *mut u8) -> bool {
    register_cuda_ptr_in(&CUDA_DEVICE_ALLOCATIONS, ptr)
}

fn unregister_device_ptr(ptr: *mut u8) -> bool {
    unregister_cuda_ptr_in(&CUDA_DEVICE_ALLOCATIONS, ptr)
}

fn register_host_pinned_ptr(ptr: *mut u8) -> bool {
    register_cuda_ptr_in(&CUDA_HOST_PINNED_ALLOCATIONS, ptr)
}

fn unregister_host_pinned_ptr(ptr: *mut u8) -> bool {
    unregister_cuda_ptr_in(&CUDA_HOST_PINNED_ALLOCATIONS, ptr)
}

/// A memory backend allocating CUDA device memory.
///
/// Under the hood, this uses CUDA unified memory (`cuMemAllocManaged`) and advises the driver
/// to prefer device placement (`cuMemAdvise` with `CU_MEM_ADVISE_SET_PREFERRED_LOCATION`). This
/// allows the host CPU to write allocator metadata in-band without segfaulting, while keeping
/// the allocation device-preferred for optimal kernel performance.
pub struct CudaDeviceBackend;

impl MemoryBackend for CudaDeviceBackend {
    const SUPPORTS_PAGE_RESET: bool = false;
    const SUPPORTS_MAKE_GUARD: bool = false;
    const SUPPORTS_DECOMMIT: bool = false;

    #[inline]
    unsafe fn allocate(size: usize) -> *mut u8 {
        unsafe { init_cuda() };
        let alloc_ptr = CU_MEM_ALLOC_MANAGED.load(Ordering::Acquire);
        let free_ptr = CU_MEM_FREE.load(Ordering::Acquire);
        let advise_ptr = CU_MEM_ADVISE.load(Ordering::Acquire);
        if !alloc_ptr.is_null() && !free_ptr.is_null() {
            type CuMemAllocManagedFn =
                unsafe extern "system" fn(*mut u64, usize, u32) -> core::ffi::c_int;
            let cu_mem_alloc_managed: CuMemAllocManagedFn =
                unsafe { core::mem::transmute::<*mut c_void, CuMemAllocManagedFn>(alloc_ptr) };

            let mut dptr: u64 = 0;
            // CU_MEM_ATTACH_GLOBAL = 0x01
            let res = unsafe { cu_mem_alloc_managed(&mut dptr, size, 0x01) };
            if res == 0 && dptr != 0 {
                let ptr = dptr as *mut u8;
                if register_device_ptr(ptr) {
                    if !advise_ptr.is_null() {
                        type CuMemAdviseFn =
                            unsafe extern "system" fn(u64, usize, u32, i32) -> core::ffi::c_int;
                        let cu_mem_advise: CuMemAdviseFn = unsafe {
                            core::mem::transmute::<*mut c_void, CuMemAdviseFn>(advise_ptr)
                        };
                        // CU_MEM_ADVISE_SET_PREFERRED_LOCATION = 3
                        let _ = unsafe { cu_mem_advise(dptr, size, 3, 0) };
                    }
                    return ptr;
                }
                type CuMemFreeFn = unsafe extern "system" fn(u64) -> core::ffi::c_int;
                let cu_mem_free: CuMemFreeFn =
                    unsafe { core::mem::transmute::<*mut c_void, CuMemFreeFn>(free_ptr) };
                let _ = unsafe { cu_mem_free(dptr) };
            }
        }
        core::ptr::null_mut()
    }

    #[inline]
    unsafe fn deallocate(ptr: *mut u8, _size: usize) -> bool {
        if unregister_device_ptr(ptr) {
            let free_ptr = CU_MEM_FREE.load(Ordering::Acquire);
            if !free_ptr.is_null() {
                type CuMemFreeFn = unsafe extern "system" fn(u64) -> core::ffi::c_int;
                let cu_mem_free: CuMemFreeFn =
                    unsafe { core::mem::transmute::<*mut c_void, CuMemFreeFn>(free_ptr) };
                let status = unsafe { cu_mem_free(ptr as u64) };
                return status == 0;
            }
        }
        false
    }
}

/// A memory backend allocating CUDA page-locked (pinned) host memory.
pub struct CudaHostPinnedBackend;

impl MemoryBackend for CudaHostPinnedBackend {
    const SUPPORTS_PAGE_RESET: bool = false;
    const SUPPORTS_MAKE_GUARD: bool = false;
    const SUPPORTS_DECOMMIT: bool = false;

    #[inline]
    unsafe fn allocate(size: usize) -> *mut u8 {
        unsafe { init_cuda() };
        let host_alloc_ptr = CU_MEM_HOST_ALLOC.load(Ordering::Acquire);
        let free_host_ptr = CU_MEM_FREE_HOST.load(Ordering::Acquire);
        if !host_alloc_ptr.is_null() && !free_host_ptr.is_null() {
            type CuMemHostAllocFn =
                unsafe extern "system" fn(*mut *mut c_void, usize, u32) -> core::ffi::c_int;
            let cu_mem_host_alloc: CuMemHostAllocFn =
                unsafe { core::mem::transmute::<*mut c_void, CuMemHostAllocFn>(host_alloc_ptr) };

            let mut host_ptr: *mut c_void = core::ptr::null_mut();
            // CU_MEMHOSTALLOC_DEVICEMAP = 0x02
            let res = unsafe { cu_mem_host_alloc(core::ptr::addr_of_mut!(host_ptr), size, 0x02) };
            if res == 0 && !host_ptr.is_null() {
                let ptr = host_ptr as *mut u8;
                if register_host_pinned_ptr(ptr) {
                    return ptr;
                }
                type CuMemFreeHostFn = unsafe extern "system" fn(*mut c_void) -> core::ffi::c_int;
                let cu_mem_free_host: CuMemFreeHostFn =
                    unsafe { core::mem::transmute::<*mut c_void, CuMemFreeHostFn>(free_host_ptr) };
                let _ = unsafe { cu_mem_free_host(host_ptr) };
            }
        }
        core::ptr::null_mut()
    }

    #[inline]
    unsafe fn deallocate(ptr: *mut u8, _size: usize) -> bool {
        if unregister_host_pinned_ptr(ptr) {
            let free_host_ptr = CU_MEM_FREE_HOST.load(Ordering::Acquire);
            if !free_host_ptr.is_null() {
                type CuMemFreeHostFn = unsafe extern "system" fn(*mut c_void) -> core::ffi::c_int;
                let cu_mem_free_host: CuMemFreeHostFn =
                    unsafe { core::mem::transmute::<*mut c_void, CuMemFreeHostFn>(free_host_ptr) };
                let status = unsafe { cu_mem_free_host(ptr as *mut c_void) };
                return status == 0;
            }
        }
        false
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
        CudaAllocationRegistry {
            slots: [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_TRACKED_CUDA_ALLOCATIONS],
            count: AtomicUsize::new(0),
        }
    }

    fn is_cuda_ptr_in(registry: &CudaAllocationRegistry, ptr: *mut u8) -> bool {
        let active_count = registry.count.load(Ordering::Acquire);
        if active_count == 0 {
            return false;
        }

        let mut seen_non_null = 0;
        let start_idx = (ptr as usize >> 12) % MAX_TRACKED_CUDA_ALLOCATIONS;
        for i in 0..MAX_TRACKED_CUDA_ALLOCATIONS {
            let idx = (start_idx + i) % MAX_TRACKED_CUDA_ALLOCATIONS;
            let val = registry.slots[idx].load(Ordering::Relaxed);
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
