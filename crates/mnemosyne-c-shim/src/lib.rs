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
/// released with [`mnemosyne_free`] (exported as `free`).
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
/// a multiple of `alignment` (per the C11 contract).
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
/// Returns `0` on success, `EINVAL` when `alignment` is not a power-of-two
/// multiple of `size_of::<*mut c_void>()`, or `ENOMEM` on allocation
/// failure. `*memptr` is only written on success.
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

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ALLOC_HOOK_CALLED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
    static FREE_HOOK_CALLED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

    unsafe extern "C" fn test_alloc_hook(ptr: *mut c_void, size: usize) {
        if !ptr.is_null() && size > 0 {
            ALLOC_HOOK_CALLED.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        }
    }

    unsafe extern "C" fn test_free_hook(ptr: *mut c_void, size: usize) {
        if !ptr.is_null() && size > 0 {
            FREE_HOOK_CALLED.fetch_add(1, core::sync::atomic::Ordering::SeqCst);
        }
    }

    #[test]
    fn test_c_shim_profiling_and_hooks() {
        let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
        unsafe {
            mnemosyne_reset_profiler_for_testing();
            ALLOC_HOOK_CALLED.store(0, core::sync::atomic::Ordering::SeqCst);
            FREE_HOOK_CALLED.store(0, core::sync::atomic::Ordering::SeqCst);

            mnemosyne_register_alloc_hook(Some(test_alloc_hook));
            mnemosyne_register_free_hook(Some(test_free_hook));

            let ptr = malloc(32);
            assert!(!ptr.is_null());
            assert_eq!(ALLOC_HOOK_CALLED.load(core::sync::atomic::Ordering::SeqCst), 1);

            free(ptr);
            assert_eq!(FREE_HOOK_CALLED.load(core::sync::atomic::Ordering::SeqCst), 1);

            mnemosyne_register_alloc_hook(None);
            mnemosyne_register_free_hook(None);
        }
    }

    // The shim shares process-wide allocator state with every other test
    // in the workspace; serialize the shim tests among themselves so their
    // own assertions stay deterministic.
    static SHIM_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn malloc_free_round_trip_is_aligned_and_writable() {
        let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
        let ptr = unsafe { malloc(64) };
        assert!(!ptr.is_null(), "malloc(64) returned null");
        assert_eq!(
            ptr as usize % MALLOC_ALIGN,
            0,
            "malloc result not aligned to {MALLOC_ALIGN}"
        );
        unsafe {
            (ptr as *mut u8).write(0xAB);
            assert_eq!((ptr as *mut u8).read(), 0xAB);
            free(ptr);
        }
    }

    #[test]
    fn malloc_zero_returns_unique_freeable_pointer() {
        let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
        let ptr = unsafe { malloc(core::hint::black_box(0)) };
        assert!(!ptr.is_null(), "malloc(0) should return a unique pointer");
        unsafe { free(ptr) };
    }

    #[test]
    fn free_null_is_a_no_op() {
        let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
        unsafe { free(core::hint::black_box(core::ptr::null_mut())) };
    }

    #[test]
    fn calloc_zero_initializes_the_requested_span() {
        let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
        let n = 16usize;
        let elem = 8usize;
        let ptr = unsafe { calloc(n, elem) } as *mut u8;
        assert!(!ptr.is_null());
        for i in 0..(n * elem) {
            assert_eq!(unsafe { ptr.add(i).read() }, 0, "calloc byte {i} not zero");
        }
        unsafe { free(ptr as *mut c_void) };
    }

    #[test]
    fn calloc_overflow_returns_null() {
        let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
        let ptr = unsafe { calloc(core::hint::black_box(usize::MAX), core::hint::black_box(2)) };
        let ptr = core::hint::black_box(ptr);
        assert!(ptr.is_null(), "calloc overflow must return null");
    }

    #[test]
    fn realloc_null_acts_as_malloc() {
        let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
        let ptr = unsafe {
            realloc(
                core::hint::black_box(core::ptr::null_mut()),
                core::hint::black_box(32),
            )
        };
        assert!(!ptr.is_null());
        unsafe { free(ptr) };
    }

    #[test]
    fn realloc_zero_frees_and_returns_null() {
        let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
        let ptr = unsafe { malloc(core::hint::black_box(32)) };
        assert!(!ptr.is_null());
        let out = unsafe { realloc(core::hint::black_box(ptr), core::hint::black_box(0)) };
        let out = core::hint::black_box(out);
        assert!(out.is_null(), "realloc(_, 0) must return null");
    }

    #[test]
    fn realloc_preserves_bytes_across_grow() {
        let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
        let ptr = unsafe { malloc(16) } as *mut u8;
        assert!(!ptr.is_null());
        for i in 0..16usize {
            unsafe { ptr.add(i).write((i as u8).wrapping_add(0x10)) };
        }
        let grown = unsafe { realloc(ptr as *mut c_void, 4096) } as *mut u8;
        assert!(!grown.is_null());
        for i in 0..16usize {
            assert_eq!(
                unsafe { grown.add(i).read() },
                (i as u8).wrapping_add(0x10),
                "realloc grow did not preserve byte {i}"
            );
        }
        unsafe { free(grown as *mut c_void) };
    }

    #[test]
    fn aligned_alloc_honors_alignment_and_rejects_misuse() {
        let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
        let bad = unsafe { aligned_alloc(core::hint::black_box(64), core::hint::black_box(100)) };
        let bad = core::hint::black_box(bad);
        assert!(
            bad.is_null(),
            "aligned_alloc with non-multiple size must fail"
        );

        let good = unsafe { aligned_alloc(64, 128) };
        assert!(!good.is_null());
        assert_eq!(good as usize % 64, 0, "aligned_alloc result not 64-aligned");
        unsafe { free(good) };
    }

    #[test]
    fn posix_memalign_sets_pointer_and_validates_alignment() {
        let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
        let mut out: *mut c_void = core::ptr::null_mut();
        // Alignment not a multiple of sizeof(void*) bound — 1 is below the
        // pointer size, so EINVAL.
        let rc_bad = unsafe { posix_memalign(&mut out as *mut *mut c_void, 1, 64) };
        assert_eq!(
            rc_bad, EINVAL,
            "posix_memalign accepted sub-pointer alignment"
        );

        let rc = unsafe { posix_memalign(&mut out as *mut *mut c_void, 64, 256) };
        assert_eq!(rc, 0, "posix_memalign returned error for valid request");
        assert!(!out.is_null());
        assert_eq!(out as usize % 64, 0, "posix_memalign result not 64-aligned");
        unsafe { free(out) };
    }

    #[test]
    fn malloc_usable_size_reports_at_least_request() {
        let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
        let ptr = unsafe { malloc(40) };
        assert!(!ptr.is_null());
        let usable = unsafe { malloc_usable_size(ptr) };
        assert!(usable >= 40, "malloc_usable_size {usable} below request 40");
        assert_eq!(
            unsafe { malloc_usable_size(core::ptr::null_mut()) },
            0,
            "malloc_usable_size(null) must be 0"
        );
        unsafe { free(ptr) };
    }
}
