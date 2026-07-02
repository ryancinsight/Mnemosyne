//! Windows vectored-exception machinery isolating the `cuInit` probe.
//!
//! Broken or stub NVIDIA driver stacks (headless nodes, display-less remote
//! sessions) can raise access violations or in-page errors during `cuInit`,
//! either on the probe worker thread or on a helper thread the driver spawns
//! internally. To keep such a fault from taking the process down while still
//! surfacing it as "CUDA unavailable", the probe:
//!
//! 1. installs a vectored exception handler only for the duration of the
//!    probe window and removes it immediately afterwards — the handler is
//!    never left installed process-wide;
//! 2. gates the handler body with an atomic probe-active flag so a fault
//!    outside the window (or racing with installation) is never converted and
//!    continues the normal search — genuine crashes elsewhere in the process
//!    stay crashes;
//! 3. never terminates the process from the handler. A qualifying fault
//!    (in-page error, or access violation inside `nvcuda.dll`) is recorded as
//!    a probe failure and only the faulting thread is terminated, by
//!    redirecting its instruction pointer to [`probe_thread_exit`] (which
//!    calls `ExitThread(1)`) and resuming execution. The main thread's join
//!    then observes a nonzero worker exit code (or a timeout when a driver
//!    helper thread faulted and the worker hangs) and reports `cuInit`
//!    failure, so the backends mark CUDA unavailable and the process
//!    survives.
//!
//! Redirect constraint: the landing routine is entered by rewriting `Rip`,
//! not by a `call`, so it must not rely on the faulting frame's stack
//! contents. The handler manufactures a fresh call-entry stack alignment
//! (`Rsp % 16 == 8`) below the faulting frame, and the landing routine only
//! performs the outgoing `ExitThread` call. The redirect is implemented for
//! `x86_64` (the only architecture with a Windows CUDA driver); on other
//! architectures the handler declines and the fault propagates as a real
//! crash rather than being masked.
//!
//! Residual risk (documented, accepted): a thread terminated mid-`cuInit`
//! may leave driver-internal locks held, and a faulted helper thread can
//! leave the worker blocked past the probe timeout, leaking that worker
//! thread. Both outcomes are confined to a process in which the driver has
//! already proven itself broken; CUDA is reported unavailable and no further
//! driver calls are issued by these backends.

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};

#[repr(C)]
struct EXCEPTION_RECORD {
    exception_code: u32,
    exception_flags: u32,
    exception_record: *mut EXCEPTION_RECORD,
    exception_address: *mut c_void,
    number_parameters: u32,
    exception_information: [usize; 15],
}

#[repr(C)]
struct EXCEPTION_POINTERS {
    exception_record: *mut EXCEPTION_RECORD,
    context_record: *mut c_void,
}

extern "system" {
    fn GetModuleHandleA(lpModuleName: *const u8) -> *mut c_void;
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
    fn ExitThread(dwExitCode: u32) -> !;
}

const STATUS_ACCESS_VIOLATION: u32 = 0xC000_0005;
const STATUS_IN_PAGE_ERROR: u32 = 0xC000_0006;
const EXCEPTION_CONTINUE_SEARCH: i32 = 0;
#[cfg(target_arch = "x86_64")]
const EXCEPTION_CONTINUE_EXECUTION: i32 = -1;
/// Probe join budget: `cuInit` on a healthy driver completes well within
/// this; a hung worker past it is treated as a probe failure.
const PROBE_TIMEOUT_MS: u32 = 5000;
/// Sentinel `cuInit` result while the probe is pending or after it faulted;
/// any nonzero value marks CUDA unavailable.
const CU_INIT_FAILED: i32 = -1;

/// `Rsp` offset inside the x86_64 Windows `CONTEXT` record.
#[cfg(target_arch = "x86_64")]
const CONTEXT_RSP_OFFSET: usize = 0x98;
/// `Rip` offset inside the x86_64 Windows `CONTEXT` record.
#[cfg(target_arch = "x86_64")]
const CONTEXT_RIP_OFFSET: usize = 0xF8;

/// True only while the isolated `cuInit` probe window is open. Gates
/// [`probe_veh_handler`] so a fault outside the window is never converted:
/// even if handler installation races with an unrelated fault, the handler
/// falls through to `EXCEPTION_CONTINUE_SEARCH`.
static PROBE_ACTIVE: AtomicBool = AtomicBool::new(false);
/// `cuInit` symbol handed to the worker thread. Release store on the
/// spawning thread pairs with the worker's Acquire load across the
/// `CreateThread` boundary.
static CU_INIT_PTR: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// Worker-reported `cuInit` result. Release store on the worker (or the
/// exception handler) pairs with the main thread's Acquire load after the
/// join.
static CU_INIT_RESULT: AtomicI32 = AtomicI32::new(CU_INIT_FAILED);

unsafe extern "system" fn worker_thread_fn(_param: *mut c_void) -> u32 {
    type CuInitFn = unsafe extern "system" fn(u32) -> i32;
    // SAFETY: `CU_INIT_PTR` was stored (Release) by `run_cu_init_isolated`
    // before this thread was created and holds the resolved non-null `cuInit`
    // export, whose ABI matches `CuInitFn`.
    let cu_init: CuInitFn = unsafe { core::mem::transmute(CU_INIT_PTR.load(Ordering::Acquire)) };
    // SAFETY: `cuInit(0)` is the documented driver initialization call.
    let res = unsafe { cu_init(0) };
    CU_INIT_RESULT.store(res, Ordering::Release);
    0
}

/// Returns true when `addr` lies inside the loaded `nvcuda.dll` image.
unsafe fn is_address_in_nvcuda(addr: *mut c_void) -> bool {
    // SAFETY: `lpModuleName` is a valid NUL-terminated string; the call does
    // not take a reference on the module.
    let h_module = unsafe { GetModuleHandleA(c"nvcuda.dll".as_ptr() as *const u8) };
    if h_module.is_null() {
        return false;
    }
    let base = h_module as usize;
    let addr_val = addr as usize;
    if addr_val < base {
        return false;
    }
    // SAFETY: `h_module` is the base of a loaded PE image, so the DOS header
    // (`e_lfanew` at 0x3C) and the NT `OptionalHeader.SizeOfImage` field
    // (NT signature + FileHeader = 24 bytes, SizeOfImage at offset 56 of the
    // OptionalHeader) are mapped and readable.
    unsafe {
        let dos_header = base as *const u8;
        let e_lfanew = *(dos_header.add(0x3c) as *const i32) as usize;
        let nt_headers = base + e_lfanew;
        let size_of_image = *((nt_headers + 24 + 56) as *const u32) as usize;
        addr_val < base + size_of_image
    }
}

/// VEH redirect target: terminates only the faulting thread with exit code 1.
///
/// Entered by an instruction-pointer rewrite, not by `call`, so it must not
/// rely on the faulting frame's stack contents. It executes on the fresh
/// call-entry stack alignment manufactured by [`probe_veh_handler`] and only
/// performs the outgoing `ExitThread` call.
#[cfg(target_arch = "x86_64")]
unsafe extern "system" fn probe_thread_exit() -> ! {
    // SAFETY: `ExitThread` terminates the current thread and does not return;
    // it reads nothing from the (discarded) faulting frame.
    unsafe { ExitThread(1) }
}

/// Vectored handler active only during the `cuInit` probe window.
///
/// Converts a qualifying probe fault into termination of the faulting thread
/// (never the process) and declines everything else.
unsafe extern "system" fn probe_veh_handler(exception_info: *mut EXCEPTION_POINTERS) -> i32 {
    if !PROBE_ACTIVE.load(Ordering::Acquire) {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    // SAFETY: the OS passes a valid EXCEPTION_POINTERS with live record and
    // context pointers for the duration of the handler call.
    let (code, addr) = unsafe {
        let record = (*exception_info).exception_record;
        ((*record).exception_code, (*record).exception_address)
    };

    // Qualifying probe faults: Unified Memory in-page errors raised by a
    // stub/headless driver, or access violations inside nvcuda.dll itself.
    // Everything else continues the normal search even inside the window.
    let is_probe_fault = code == STATUS_IN_PAGE_ERROR
        || (code == STATUS_ACCESS_VIOLATION
            // SAFETY: `addr` originates from the exception record.
            && unsafe { is_address_in_nvcuda(addr) });
    if !is_probe_fault {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    CU_INIT_RESULT.store(CU_INIT_FAILED, Ordering::Release);

    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: the context record is a live, writable x86_64 CONTEXT for
        // the faulting thread; Rsp/Rip live at the documented fixed offsets.
        // Rewriting them and returning EXCEPTION_CONTINUE_EXECUTION resumes
        // the faulting thread at `probe_thread_exit` on a manufactured
        // call-entry stack (16-byte aligned minus 8, below the faulting
        // frame), which terminates only that thread.
        unsafe {
            let context = (*exception_info).context_record as *mut u8;
            let rsp = context.add(CONTEXT_RSP_OFFSET) as *mut u64;
            let rip = context.add(CONTEXT_RIP_OFFSET) as *mut u64;
            *rsp = (*rsp & !0xF).wrapping_sub(8);
            *rip = probe_thread_exit as *const () as usize as u64;
        }
        EXCEPTION_CONTINUE_EXECUTION
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        // No CONTEXT redirect is implemented for this architecture (no
        // Windows CUDA driver exists for it); decline so the fault propagates
        // as a genuine crash instead of being masked.
        EXCEPTION_CONTINUE_SEARCH
    }
}

/// Runs `cuInit(0)` on a dedicated worker thread under the probe-window
/// vectored exception handler. Returns the `cuInit` result, or a nonzero
/// failure code when the probe faulted, hung, or could not be set up.
///
/// # Safety
///
/// `init_sym` must be the resolved, non-null `cuInit` export. The caller must
/// hold exclusive ownership of the one-time initialization phase (enforced by
/// `init_cuda`'s state machine), which also serializes probe windows.
pub(super) unsafe fn run_cu_init_isolated(init_sym: *mut c_void) -> i32 {
    CU_INIT_PTR.store(init_sym, Ordering::Release);
    CU_INIT_RESULT.store(CU_INIT_FAILED, Ordering::Release);

    // SAFETY: `probe_veh_handler` matches the required VEH signature and
    // remains valid for the (bounded) installation window below.
    let handler = unsafe { AddVectoredExceptionHandler(1, probe_veh_handler) };
    if handler.is_null() {
        return CU_INIT_FAILED;
    }
    PROBE_ACTIVE.store(true, Ordering::Release);

    let mut thread_id = 0_u32;
    // SAFETY: `worker_thread_fn` matches the thread-start signature and takes
    // no parameter; default security attributes and stack size are valid.
    let thread_handle = unsafe {
        CreateThread(
            core::ptr::null_mut(),
            0,
            worker_thread_fn,
            core::ptr::null_mut(),
            0,
            &mut thread_id,
        )
    };
    if thread_handle.is_null() {
        PROBE_ACTIVE.store(false, Ordering::Release);
        // SAFETY: `handler` is the live registration returned above.
        unsafe { RemoveVectoredExceptionHandler(handler) };
        return CU_INIT_FAILED;
    }

    // SAFETY: `thread_handle` is a live thread handle owned by this frame;
    // the bounded wait tolerates a hung worker (treated as probe failure).
    let mut exit_code = 0_u32;
    unsafe {
        WaitForSingleObject(thread_handle, PROBE_TIMEOUT_MS);
        GetExitCodeThread(thread_handle, &mut exit_code);
        CloseHandle(thread_handle);
    }

    // Close the probe window before removing the handler so no thread can
    // observe an installed-but-ungated handler.
    PROBE_ACTIVE.store(false, Ordering::Release);
    // SAFETY: `handler` is the live registration returned above.
    unsafe { RemoveVectoredExceptionHandler(handler) };

    if exit_code == 0 {
        CU_INIT_RESULT.load(Ordering::Acquire)
    } else {
        // Nonzero: the worker was redirected to `probe_thread_exit` (exit
        // code 1) or is still running after the timeout (`STILL_ACTIVE`).
        CU_INIT_FAILED
    }
}
