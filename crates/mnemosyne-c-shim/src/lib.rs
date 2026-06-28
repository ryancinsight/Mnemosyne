//! C ABI shim exposing Mnemosyne through the standard `malloc` family.
//!
//! The functions here mirror the C standard / POSIX allocator surface so
//! Mnemosyne can be used from C/C++ code or interposed via `LD_PRELOAD`
//! (Unix) / DLL injection (Windows). They route to the same thread-local
//! allocator the Rust `#[global_allocator]` path uses, through
//! `mnemosyne_local::{thread_alloc, thread_free, usable_size}` under the
//! standard policy and OS-mapping backend.
//!
//! ## C vs. Rust copy-length semantics
//!
//! The Rust `GlobalAlloc::realloc` path copies only `layout.size()` bytes
//! because the Rust contract tracks the originally-requested size. C has
//! no such tracking: `realloc` must preserve the lesser of the *old usable
//! size* and the new size, because a C caller may legitimately have
//! written the entire usable region returned by `malloc`. The shim's
//! `realloc` therefore copies `min(usable_size(old), new_size)` — the
//! correct and safe choice for C semantics, and distinct from the Rust
//! path on purpose.

#![no_std]

use core::ffi::c_void;
use mnemosyne_backend::MemoryBackendWrapper;
use mnemosyne_core::StandardPolicy;
use mnemosyne_local::{thread_alloc, thread_free, usable_size};

/// Minimum alignment the C `malloc`/`calloc`/`realloc` family must
/// guarantee. The C standard requires the result to be suitably aligned
/// for any object with a fundamental alignment requirement; on every
/// supported 64-bit target that is `max_align_t == 16`.
const MALLOC_ALIGN: usize = 16;

/// POSIX `EINVAL`, returned by `posix_memalign` for an invalid alignment.
const EINVAL: i32 = 22;
/// POSIX `ENOMEM`, returned by `posix_memalign` when the allocation fails.
const ENOMEM: i32 = 12;

/// Allocates `size` bytes aligned to at least `MALLOC_ALIGN`.
///
/// Returns `NULL` on failure. A zero-size request allocates a minimum
/// 1-byte block so the returned pointer is unique and freeable, matching
/// the common glibc/jemalloc behavior (the C standard permits either a
/// null or a unique pointer for `malloc(0)`; a unique pointer avoids
/// surprising callers that treat null as failure).
///
/// # Safety
///
/// This is an `extern "C"` entry point. The returned pointer must be
/// released with [`free`].
#[no_mangle]
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    let request = if size == 0 { 1 } else { size };
    // Safety: MALLOC_ALIGN is a nonzero power of two; thread_alloc validates
    // the request and returns null on failure.
    unsafe {
        thread_alloc::<StandardPolicy, MemoryBackendWrapper>(request, MALLOC_ALIGN) as *mut c_void
    }
}

/// Releases a block previously returned by [`malloc`], [`calloc`],
/// [`realloc`], [`aligned_alloc`], or [`posix_memalign`].
///
/// A null pointer is ignored, matching `free(NULL)` semantics.
///
/// # Safety
///
/// `ptr` must be null or a pointer returned by this shim and not yet
/// freed.
#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut c_void) {
    // thread_free is pointer-only (it derives the owning page/segment) and
    // tolerates null, so no layout is needed here.
    unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr as *mut u8) };
}

/// Allocates `nmemb * size` zero-initialized bytes.
///
/// Returns `NULL` on multiplication overflow or allocation failure.
///
/// # Safety
///
/// `extern "C"` entry point; release with [`free`].
#[no_mangle]
pub unsafe extern "C" fn calloc(nmemb: usize, size: usize) -> *mut c_void {
    let Some(total) = nmemb.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    let request = if total == 0 { 1 } else { total };
    // Safety: MALLOC_ALIGN is a valid alignment; thread_alloc returns null on failure.
    let ptr =
        unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(request, MALLOC_ALIGN) };
    if !ptr.is_null() {
        // Zero only the requested span. The user observes `total` bytes;
        // the size-class slack beyond it is irrelevant to the caller.
        // Safety: ptr is valid for writes of `total` bytes (>= request).
        unsafe { core::ptr::write_bytes(ptr, 0, total) };
    }
    ptr as *mut c_void
}

/// Resizes the allocation at `ptr` to `new_size` bytes.
///
/// - `realloc(NULL, n)` behaves as `malloc(n)`.
/// - `realloc(p, 0)` frees `p` and returns `NULL`.
/// - Otherwise returns `ptr` unchanged when the new size still fits the
///   current usable size, or allocates a new block, copies
///   `min(usable_size(ptr), new_size)` bytes, frees the old block, and
///   returns the new pointer.
///
/// # Safety
///
/// `ptr` must be null or a live pointer from this shim; release the
/// result with [`free`].
#[no_mangle]
pub unsafe extern "C" fn realloc(ptr: *mut c_void, new_size: usize) -> *mut c_void {
    if ptr.is_null() {
        return unsafe { malloc(new_size) };
    }
    if new_size == 0 {
        unsafe { free(ptr) };
        return core::ptr::null_mut();
    }

    // Safety: ptr is a live allocation from this shim.
    let current_usable = unsafe { usable_size(ptr as *mut u8) };
    if new_size <= current_usable {
        // The existing block already satisfies the request.
        return ptr;
    }

    let new_ptr = unsafe { malloc(new_size) };
    if !new_ptr.is_null() {
        // C semantics: preserve the lesser of the old usable region and the
        // new size. The `new_size <= current_usable` case already returned
        // above, so here `new_size > current_usable` and
        // `min(current_usable, new_size)` is exactly `current_usable` — copy
        // the whole old usable region (a C caller may have written all of it).
        let copy_len = current_usable;
        // Safety: both pointers are valid for `copy_len` bytes and do not
        // overlap (malloc returned a fresh block).
        unsafe {
            core::ptr::copy_nonoverlapping(ptr as *const u8, new_ptr as *mut u8, copy_len);
            free(ptr);
        }
    }
    new_ptr
}

/// C11 `aligned_alloc`: allocates `size` bytes aligned to `alignment`.
///
/// Returns `NULL` when `alignment` is not a power of two or `size` is not
/// a multiple of `alignment` (per the C11 contract). Also returns `NULL`
/// for an `alignment` above the 2 MiB segment size, which the allocator
/// cannot satisfy.
///
/// # Safety
///
/// `extern "C"` entry point; release with [`free`].
#[no_mangle]
pub unsafe extern "C" fn aligned_alloc(alignment: usize, size: usize) -> *mut c_void {
    if alignment == 0 || !alignment.is_power_of_two() || !size.is_multiple_of(alignment) {
        return core::ptr::null_mut();
    }
    let request = if size == 0 { alignment } else { size };
    // Safety: alignment is a validated power of two; thread_alloc returns null on failure.
    unsafe {
        thread_alloc::<StandardPolicy, MemoryBackendWrapper>(request, alignment) as *mut c_void
    }
}

/// POSIX `posix_memalign`: stores an `alignment`-aligned `size`-byte
/// allocation in `*memptr`.
///
/// Returns `0` on success, `EINVAL` when `alignment` is not a power of two
/// or is below `size_of::<*mut c_void>()`, or `ENOMEM` on allocation
/// failure. A valid power-of-two `alignment` above the 2 MiB segment size
/// is unsupportable and yields `ENOMEM` (it is a valid POSIX alignment, so
/// not `EINVAL`). `*memptr` is only written on success.
///
/// # Safety
///
/// `memptr` must be a valid, writable `*mut *mut c_void`.
#[no_mangle]
pub unsafe extern "C" fn posix_memalign(
    memptr: *mut *mut c_void,
    alignment: usize,
    size: usize,
) -> i32 {
    if memptr.is_null() {
        return EINVAL;
    }
    // POSIX requires alignment to be a power of two and a multiple of
    // sizeof(void*).
    if alignment < core::mem::size_of::<*mut c_void>() || !alignment.is_power_of_two() {
        return EINVAL;
    }
    let request = if size == 0 { alignment } else { size };
    // Safety: alignment validated above; thread_alloc returns null on failure.
    let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(request, alignment) };
    if ptr.is_null() {
        return ENOMEM;
    }
    // Safety: caller guarantees memptr is a writable slot.
    unsafe { *memptr = ptr as *mut c_void };
    0
}

/// Returns the number of usable bytes in the allocation at `ptr`.
///
/// Mirrors glibc/jemalloc `malloc_usable_size`. Returns `0` for a null
/// pointer.
///
/// # Safety
///
/// `ptr` must be null or a live pointer from this shim.
#[no_mangle]
pub unsafe extern "C" fn malloc_usable_size(ptr: *mut c_void) -> usize {
    // Safety: usable_size tolerates null and classifies live shim pointers.
    unsafe { usable_size(ptr as *mut u8) }
}

/// Registers a custom user allocation tracing hook.
///
/// # Safety
///
/// `hook` must be a valid function pointer adhering to the C calling convention,
/// or `None` to unregister. The hook is invoked on every allocation.
#[no_mangle]
pub unsafe extern "C" fn mnemosyne_register_alloc_hook(
    hook: Option<unsafe extern "C" fn(*mut c_void, usize)>,
) {
    mnemosyne_prof::register_alloc_hook(hook);
}

/// Registers a custom user deallocation tracing hook.
///
/// # Safety
///
/// `hook` must be a valid function pointer adhering to the C calling convention,
/// or `None` to unregister. The hook is invoked on every deallocation.
#[no_mangle]
pub unsafe extern "C" fn mnemosyne_register_free_hook(
    hook: Option<unsafe extern "C" fn(*mut c_void, usize)>,
) {
    mnemosyne_prof::register_free_hook(hook);
}

/// Enables the built-in Poisson heap sampler.
#[no_mangle]
pub extern "C" fn mnemosyne_enable_profiling(sample_interval: usize) {
    mnemosyne_prof::enable_profiling(sample_interval);
}

/// Disables the built-in Poisson heap sampler.
#[no_mangle]
pub extern "C" fn mnemosyne_disable_profiling() {
    mnemosyne_prof::disable_profiling();
}

/// Returns whether the built-in heap sampler is currently active.
#[no_mangle]
pub extern "C" fn mnemosyne_is_profiling_enabled() -> i32 {
    if mnemosyne_prof::is_profiling_enabled() {
        1
    } else {
        0
    }
}

/// Dumps a folded stack profile of active memory allocations to a file.
///
/// Returns 0 on success, or -1 on error.
///
/// # Safety
///
/// `path` must be a valid null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn mnemosyne_dump_profile(path: *const core::ffi::c_char) -> i32 {
    if path.is_null() {
        return -1;
    }
    // Safety: path must be a valid null-terminated C string.
    let c_str = unsafe { core::ffi::CStr::from_ptr(path) };
    let Ok(str_slice) = c_str.to_str() else {
        return -1;
    };
    match mnemosyne_prof::dump_profile(str_slice) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Resets the profiler state, trace hooks, and sampled data. Intended for testing.
#[no_mangle]
pub extern "C" fn mnemosyne_reset_profiler_for_testing() {
    mnemosyne_prof::reset_profiler_for_testing();
}

/// Enables the built-in memory leak detector, tracking every allocation with its backtrace.
#[no_mangle]
pub extern "C" fn mnemosyne_enable_leak_detector() {
    mnemosyne_prof::enable_leak_detector();
}

/// Disables the built-in memory leak detector.
#[no_mangle]
pub extern "C" fn mnemosyne_disable_leak_detector() {
    mnemosyne_prof::disable_leak_detector();
}

/// Returns whether the memory leak detector is currently active (1 if active, 0 if inactive).
#[no_mangle]
pub extern "C" fn mnemosyne_is_leak_detector_enabled() -> i32 {
    if mnemosyne_prof::is_leak_detector_enabled() {
        1
    } else {
        0
    }
}

/// Dumps a report of all active memory allocations (leaks) to the specified file path.
///
/// Returns the number of leaks written on success, or -1 on error.
///
/// # Safety
///
/// `path` must be a valid, null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn mnemosyne_dump_leaks(path: *const core::ffi::c_char) -> i32 {
    if path.is_null() {
        return -1;
    }
    // Safety: path must be a valid null-terminated C string.
    let c_str = unsafe { core::ffi::CStr::from_ptr(path) };
    let Ok(str_slice) = c_str.to_str() else {
        return -1;
    };
    match mnemosyne_prof::dump_leaks(str_slice) {
        Ok(count) => count as i32,
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests;
