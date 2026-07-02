//! Dynamic loading of the CUDA driver library and one-time symbol resolution.
//!
//! Owns the shared loader state: the driver library handle is resolved once
//! and cached process-wide, driver entry points are resolved through
//! [`resolve_sym`], and [`init_cuda`] gates the one-time symbol resolution
//! behind an atomic state machine so concurrent first callers observe exactly
//! one initialization.

use core::ffi::{CStr, c_void};
use core::sync::atomic::{AtomicPtr, AtomicU8, Ordering};

#[cfg(target_family = "windows")]
unsafe extern "system" {
    fn LoadLibraryA(lpLibFileName: *const u8) -> *mut c_void;
    fn GetProcAddress(hModule: *mut c_void, lpProcName: *const u8) -> *mut c_void;
    fn SwitchToThread() -> i32;
}

#[cfg(target_family = "unix")]
unsafe extern "C" {
    fn dlopen(filename: *const u8, flag: core::ffi::c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const u8) -> *mut c_void;
    fn sched_yield() -> core::ffi::c_int;
}

#[cfg(target_family = "unix")]
const RTLD_LAZY: core::ffi::c_int = 1;

/// Cached CUDA driver library handle; loaded at most once per process and
/// never unloaded.
static CUDA_LIB: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

/// Resolved `cuMemAllocManaged` entry point (null while unavailable).
pub(super) static CU_MEM_ALLOC_MANAGED: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// Resolved `cuMemFree` entry point (null while unavailable).
pub(super) static CU_MEM_FREE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// Resolved `cuMemHostAlloc` entry point (null while unavailable).
pub(super) static CU_MEM_HOST_ALLOC: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// Resolved `cuMemFreeHost` entry point (null while unavailable).
pub(super) static CU_MEM_FREE_HOST: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// Resolved `cuMemAdvise` entry point (null while unavailable).
pub(super) static CU_MEM_ADVISE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

static CUDA_INIT_STATE: AtomicU8 = AtomicU8::new(CUDA_UNINITIALIZED);

const CUDA_UNINITIALIZED: u8 = 0;
const CUDA_INITIALIZING: u8 = 1;
const CUDA_INITIALIZED: u8 = 2;

/// Spin budget for threads that lose the initialization race. The winner
/// performs library loading plus the isolated `cuInit` probe (up to seconds),
/// so losers spin briefly for the fast already-initialized case and then
/// yield their timeslice instead of burning a core.
const INIT_SPIN_LIMIT: u32 = 1024;

/// Loads (at most once) and returns the CUDA driver library handle, or null
/// when the driver is not installed.
///
/// The handle is cached process-wide and the library is never unloaded. On a
/// first-call race both threads load the library and one cached handle wins;
/// the loser's handle refers to the same module and only leaves the OS loader
/// reference count one higher, which is harmless because the driver library
/// is retained for the process lifetime anyway.
///
/// # Safety
///
/// Performs OS dynamic library loading (`LoadLibraryA` / `dlopen`).
pub(super) unsafe fn cuda_library() -> *mut c_void {
    let cached = CUDA_LIB.load(Ordering::Acquire);
    if !cached.is_null() {
        return cached;
    }

    let lib = {
        #[cfg(target_family = "windows")]
        {
            // SAFETY: `lpLibFileName` is a valid NUL-terminated string.
            unsafe { LoadLibraryA(c"nvcuda.dll".as_ptr() as *const u8) }
        }
        #[cfg(target_family = "unix")]
        {
            // SAFETY: both filenames are valid NUL-terminated strings; the
            // versioned `.so.1` name is the fallback for systems that do not
            // install the unversioned development symlink.
            let p = unsafe { dlopen(c"libcuda.so".as_ptr() as *const u8, RTLD_LAZY) };
            if p.is_null() {
                unsafe { dlopen(c"libcuda.so.1".as_ptr() as *const u8, RTLD_LAZY) }
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

    match CUDA_LIB.compare_exchange(
        core::ptr::null_mut(),
        lib,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => lib,
        Err(existing) => existing,
    }
}

/// Resolves one exported symbol from a loaded driver library handle.
/// Returns null when the export is missing.
///
/// # Safety
///
/// `lib` must be a live library handle returned by [`cuda_library`].
pub(super) unsafe fn resolve_sym(lib: *mut c_void, name: &CStr) -> *mut c_void {
    #[cfg(target_family = "windows")]
    {
        // SAFETY: `lib` is a live module handle per the caller contract and
        // `name` is NUL-terminated.
        unsafe { GetProcAddress(lib, name.as_ptr() as *const u8) }
    }
    #[cfg(target_family = "unix")]
    {
        // SAFETY: `lib` is a live dlopen handle per the caller contract and
        // `name` is NUL-terminated.
        unsafe { dlsym(lib, name.as_ptr() as *const u8) }
    }
    #[cfg(not(any(target_family = "windows", target_family = "unix")))]
    {
        let _unsupported = (lib, name);
        core::ptr::null_mut()
    }
}

/// Yields the current thread's remaining timeslice to the OS scheduler.
fn yield_thread() {
    #[cfg(target_family = "windows")]
    {
        // The return value only reports whether another thread was ready to
        // run; there is no action to take either way.
        // SAFETY: `SwitchToThread` has no preconditions.
        let _switched = unsafe { SwitchToThread() };
    }
    #[cfg(target_family = "unix")]
    {
        // `sched_yield` cannot fail on Linux; the POSIX return value carries
        // no recovery action for a spin-wait loop.
        // SAFETY: `sched_yield` has no preconditions.
        let _yielded = unsafe { sched_yield() };
    }
    #[cfg(not(any(target_family = "windows", target_family = "unix")))]
    {
        core::hint::spin_loop();
    }
}

/// Initializes the CUDA driver by dynamically resolving driver symbols.
///
/// Safe to call concurrently from multiple threads: an atomic state machine
/// (`CUDA_INIT_STATE`) ensures `init_cuda_once` executes exactly once. Threads
/// that lose the race spin briefly and then yield until initialization
/// completes.
///
/// # Safety
///
/// Callers must guarantee that no allocator operations attempt to invoke CUDA
/// backend calls before or during this initialization phase without proper
/// synchronization.
pub(super) unsafe fn init_cuda() {
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
        // SAFETY: this thread owns the one-time CUDA symbol resolution phase.
        unsafe { init_cuda_once() };
        CUDA_INIT_STATE.store(CUDA_INITIALIZED, Ordering::Release);
        return;
    }

    let mut spins: u32 = 0;
    while CUDA_INIT_STATE.load(Ordering::Acquire) != CUDA_INITIALIZED {
        if spins < INIT_SPIN_LIMIT {
            spins += 1;
            core::hint::spin_loop();
        } else {
            yield_thread();
        }
    }
}

// On Windows `cuInit(0)` runs in fault isolation (see the `veh` module); on
// other targets the driver is called directly.
#[cfg(target_family = "windows")]
use super::veh::run_cu_init_isolated;

/// Runs `cuInit(0)` directly on the calling thread.
///
/// # Safety
///
/// `init_sym` must be the resolved `cuInit` export.
#[cfg(not(target_family = "windows"))]
unsafe fn run_cu_init_isolated(init_sym: *mut c_void) -> i32 {
    type CuInitFn = unsafe extern "system" fn(u32) -> core::ffi::c_int;
    // SAFETY: `init_sym` is the resolved `cuInit` export, whose ABI matches
    // `CuInitFn`.
    let cu_init: CuInitFn = unsafe { core::mem::transmute(init_sym) };
    // SAFETY: `cuInit(0)` is the documented driver initialization call.
    unsafe { cu_init(0) }
}

/// Resolves CUDA driver API symbols from the cached driver library and, when
/// the isolated `cuInit` probe succeeds, publishes the entry points.
///
/// # Safety
///
/// The caller must hold exclusive ownership of the one-time initialization
/// phase (enforced by `init_cuda`'s state machine).
unsafe fn init_cuda_once() {
    // SAFETY: OS library loading with no further preconditions.
    let lib = unsafe { cuda_library() };
    if lib.is_null() {
        return;
    }

    // SAFETY: `lib` is the live handle just returned by `cuda_library`.
    let init_sym = unsafe { resolve_sym(lib, c"cuInit") };
    // SAFETY: as above.
    let alloc_sym = unsafe { resolve_sym(lib, c"cuMemAllocManaged") };
    // SAFETY: as above.
    let free_sym = unsafe { resolve_sym(lib, c"cuMemFree") };
    // SAFETY: as above.
    let host_alloc_sym = unsafe { resolve_sym(lib, c"cuMemHostAlloc") };
    // SAFETY: as above.
    let free_host_sym = unsafe { resolve_sym(lib, c"cuMemFreeHost") };
    // SAFETY: as above.
    let advise_sym = unsafe { resolve_sym(lib, c"cuMemAdvise") };

    if init_sym.is_null() || alloc_sym.is_null() || free_sym.is_null() {
        return;
    }

    // SAFETY: `init_sym` is the resolved, non-null `cuInit` export.
    if unsafe { run_cu_init_isolated(init_sym) } != 0 {
        // Probe failed or faulted: leave every entry point null so all
        // backends report CUDA unavailable (allocate returns null).
        return;
    }

    CU_MEM_ALLOC_MANAGED.store(alloc_sym, Ordering::Release);
    CU_MEM_FREE.store(free_sym, Ordering::Release);
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
