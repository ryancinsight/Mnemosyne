use core::ffi::c_void;

use mnemosyne_backend::MemoryBackendWrapper;
use mnemosyne_core::StandardPolicy;
use mnemosyne_local::thread_alloc;

use crate::{EINVAL, ENOMEM};

/// C11 `aligned_alloc`: allocates `size` bytes aligned to `alignment`.
///
/// Returns `NULL` when `alignment` is not a power of two or `size` is not
/// a multiple of `alignment` (per the C11 contract). Also returns `NULL`
/// for an `alignment` above the 2 MiB segment size, which the allocator
/// cannot satisfy.
///
/// # Safety
///
/// `extern "C"` entry point; release with [`crate::free`].
// Not exported under `cfg(test)` or `cfg(fuzzing)`: on ELF targets a
// `no_mangle` definition of a libc allocator symbol interposes process-wide,
// so the test harness's — or libFuzzer's — own startup allocations would route
// here before the shim is ready and the binary dies before listing tests (or
// printing the fuzzer banner). Tests and the fuzz target call these by Rust
// path, so suppressing only the C symbol costs no coverage.
#[cfg_attr(not(any(test, fuzzing)), unsafe(no_mangle))]
pub unsafe extern "C" fn aligned_alloc(alignment: usize, size: usize) -> *mut c_void {
    if alignment == 0 || !alignment.is_power_of_two() || !size.is_multiple_of(alignment) {
        return core::ptr::null_mut();
    }
    let request = if size == 0 { alignment } else { size };
    // SAFETY: alignment is a validated power of two; thread_alloc returns null on failure.
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
// Not exported under `cfg(test)` or `cfg(fuzzing)`: on ELF targets a
// `no_mangle` definition of a libc allocator symbol interposes process-wide,
// so the test harness's — or libFuzzer's — own startup allocations would route
// here before the shim is ready and the binary dies before listing tests (or
// printing the fuzzer banner). Tests and the fuzz target call these by Rust
// path, so suppressing only the C symbol costs no coverage.
#[cfg_attr(not(any(test, fuzzing)), unsafe(no_mangle))]
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
    // SAFETY: alignment validated above; thread_alloc returns null on failure.
    let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(request, alignment) };
    if ptr.is_null() {
        return ENOMEM;
    }
    // SAFETY: caller guarantees memptr is a writable slot.
    unsafe { *memptr = ptr as *mut c_void };
    0
}
