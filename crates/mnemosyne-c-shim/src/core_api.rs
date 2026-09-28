use core::ffi::c_void;

use mnemosyne_backend::MemoryBackendWrapper;
use mnemosyne_core::StandardPolicy;
use mnemosyne_local::{thread_alloc, thread_free, usable_size};

use crate::MALLOC_ALIGN;

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
// Not exported under `cfg(test)` or `cfg(fuzzing)`: on ELF targets a
// `no_mangle` definition of a libc allocator symbol interposes process-wide,
// so the test harness's — or libFuzzer's — own startup allocations would route
// here before the shim is ready and the binary dies before listing tests (or
// printing the fuzzer banner). Tests and the fuzz target call these by Rust
// path, so suppressing only the C symbol costs no coverage.
#[cfg_attr(not(any(test, fuzzing)), unsafe(no_mangle))]
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    let request = if size == 0 { 1 } else { size };
    // SAFETY: MALLOC_ALIGN is a nonzero power of two; thread_alloc validates
    // the request and returns null on failure.
    unsafe {
        thread_alloc::<StandardPolicy, MemoryBackendWrapper>(request, MALLOC_ALIGN) as *mut c_void
    }
}

/// Releases a block previously returned by [`malloc`], [`calloc`],
/// [`realloc`], [`crate::aligned_alloc`], or [`crate::posix_memalign`].
///
/// A null pointer is ignored, matching `free(NULL)` semantics.
///
/// # Safety
///
/// `ptr` must be null or a pointer returned by this shim and not yet
/// freed.
// Not exported under `cfg(test)` or `cfg(fuzzing)`: on ELF targets a
// `no_mangle` definition of a libc allocator symbol interposes process-wide,
// so the test harness's — or libFuzzer's — own startup allocations would route
// here before the shim is ready and the binary dies before listing tests (or
// printing the fuzzer banner). Tests and the fuzz target call these by Rust
// path, so suppressing only the C symbol costs no coverage.
#[cfg_attr(not(any(test, fuzzing)), unsafe(no_mangle))]
pub unsafe extern "C" fn free(ptr: *mut c_void) {
    // thread_free is pointer-only (it derives the owning page/segment) and
    // tolerates null, so no layout is needed here.
    // SAFETY: `ptr` is either null (no-op) or a live allocation returned by
    // this shim — same contract as C `free`. thread_free validates the pointer
    // is from this allocator before touching any metadata.
    unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr as *mut u8) };
}

/// Allocates `nmemb * size` zero-initialized bytes.
///
/// Returns `NULL` on multiplication overflow or allocation failure.
///
/// # Safety
///
/// `extern "C"` entry point; release with [`free`].
// Not exported under `cfg(test)` or `cfg(fuzzing)`: on ELF targets a
// `no_mangle` definition of a libc allocator symbol interposes process-wide,
// so the test harness's — or libFuzzer's — own startup allocations would route
// here before the shim is ready and the binary dies before listing tests (or
// printing the fuzzer banner). Tests and the fuzz target call these by Rust
// path, so suppressing only the C symbol costs no coverage.
#[cfg_attr(not(any(test, fuzzing)), unsafe(no_mangle))]
pub unsafe extern "C" fn calloc(nmemb: usize, size: usize) -> *mut c_void {
    let Some(total) = nmemb.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    let request = if total == 0 { 1 } else { total };
    // SAFETY: MALLOC_ALIGN is a valid alignment; thread_alloc returns null on failure.
    let ptr =
        unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(request, MALLOC_ALIGN) };
    if !ptr.is_null() {
        // Zero only the requested span. The user observes `total` bytes;
        // the size-class slack beyond it is irrelevant to the caller.
        // SAFETY: ptr is valid for writes of `total` bytes (>= request).
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
// Not exported under `cfg(test)` or `cfg(fuzzing)`: on ELF targets a
// `no_mangle` definition of a libc allocator symbol interposes process-wide,
// so the test harness's — or libFuzzer's — own startup allocations would route
// here before the shim is ready and the binary dies before listing tests (or
// printing the fuzzer banner). Tests and the fuzz target call these by Rust
// path, so suppressing only the C symbol costs no coverage.
#[cfg_attr(not(any(test, fuzzing)), unsafe(no_mangle))]
pub unsafe extern "C" fn realloc(ptr: *mut c_void, new_size: usize) -> *mut c_void {
    if ptr.is_null() {
        // SAFETY: `new_size != 0` (guarded above) — malloc contract satisfied.
        return unsafe { malloc(new_size) };
    }
    if new_size == 0 {
        // SAFETY: `ptr` is non-null (guarded above) — free contract satisfied.
        unsafe { free(ptr) };
        return core::ptr::null_mut();
    }

    // SAFETY: ptr is a live allocation from this shim.
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
        // SAFETY: both pointers are valid for `copy_len` bytes and do not
        // overlap (malloc returned a fresh block).
        unsafe {
            core::ptr::copy_nonoverlapping(ptr as *const u8, new_ptr as *mut u8, copy_len);
            free(ptr);
        }
    }
    new_ptr
}

/// Returns the number of usable bytes in the allocation at `ptr`.
///
/// Mirrors glibc/jemalloc `malloc_usable_size`. Returns `0` for a null
/// pointer.
///
/// # Safety
///
/// `ptr` must be null or a live pointer from this shim.
// Not exported under `cfg(test)` or `cfg(fuzzing)`: on ELF targets a
// `no_mangle` definition of a libc allocator symbol interposes process-wide,
// so the test harness's — or libFuzzer's — own startup allocations would route
// here before the shim is ready and the binary dies before listing tests (or
// printing the fuzzer banner). Tests and the fuzz target call these by Rust
// path, so suppressing only the C symbol costs no coverage.
#[cfg_attr(not(any(test, fuzzing)), unsafe(no_mangle))]
pub unsafe extern "C" fn malloc_usable_size(ptr: *mut c_void) -> usize {
    // SAFETY: usable_size tolerates null and classifies live shim pointers.
    unsafe { usable_size(ptr as *mut u8) }
}
