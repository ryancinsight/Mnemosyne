//! Temporary CUDA context helpers.
//!
//! Integration tests (and probes) that must exercise the driver outside the
//! allocator's own initialization path create a short-lived context on
//! device 0 through these helpers. Library loading and symbol resolution go
//! through the shared loader state, so repeated calls do not re-open the
//! driver library.

use core::ffi::c_void;

use super::loader::{cuda_library, resolve_sym};

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
    // SAFETY: OS library loading with no further preconditions.
    let lib = unsafe { cuda_library() };
    if lib.is_null() {
        return core::ptr::null_mut();
    }

    // SAFETY: `lib` is the live handle just returned by `cuda_library`.
    let device_get = unsafe { resolve_sym(lib, c"cuDeviceGet") };
    // SAFETY: as above.
    let ctx_create = unsafe { resolve_sym(lib, c"cuCtxCreate_v2") };
    if device_get.is_null() || ctx_create.is_null() {
        return core::ptr::null_mut();
    }

    type CuDeviceGetFn = unsafe extern "system" fn(*mut i32, i32) -> i32;
    type CuCtxCreateFn = unsafe extern "system" fn(*mut *mut c_void, u32, i32) -> i32;

    // SAFETY: transmute maps the verified dynamic library symbol addresses to
    // function pointers with system calling convention.
    let cu_device_get: CuDeviceGetFn = unsafe { core::mem::transmute(device_get) };
    // SAFETY: as above.
    let cu_ctx_create: CuCtxCreateFn = unsafe { core::mem::transmute(ctx_create) };

    let mut dev: i32 = 0;
    // SAFETY: out-parameters point to live stack slots; device ordinal 0 and
    // flags 0 are the documented defaults.
    unsafe {
        if cu_device_get(&mut dev, 0) == 0 {
            let mut ctx: *mut c_void = core::ptr::null_mut();
            if cu_ctx_create(&mut ctx, 0, dev) == 0 {
                return ctx;
            }
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
    // SAFETY: OS library loading with no further preconditions.
    let lib = unsafe { cuda_library() };
    if lib.is_null() {
        return;
    }

    // SAFETY: `lib` is the live handle just returned by `cuda_library`.
    let ctx_destroy = unsafe { resolve_sym(lib, c"cuCtxDestroy_v2") };
    if !ctx_destroy.is_null() {
        type CuCtxDestroyFn = unsafe extern "system" fn(*mut c_void) -> i32;
        // SAFETY: transmute maps the verified dynamic library symbol address
        // to a function pointer with system calling convention.
        let cu_ctx_destroy: CuCtxDestroyFn = unsafe { core::mem::transmute(ctx_destroy) };
        // Destruction is best-effort teardown of a test context: a nonzero
        // status leaves nothing recoverable for the caller, so the status is
        // bound and dropped by contract of this helper.
        // SAFETY: `ctx` is a live context per the caller contract.
        let _destroy_status = unsafe { cu_ctx_destroy(ctx) };
    }
}
