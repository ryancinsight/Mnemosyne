extern crate std;

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

        let ptr = malloc(core::hint::black_box(32));
        let ptr = core::hint::black_box(ptr);
        assert!(!ptr.is_null());
        assert_eq!(
            ALLOC_HOOK_CALLED.load(core::sync::atomic::Ordering::SeqCst),
            1
        );

        free(core::hint::black_box(ptr));
        assert_eq!(
            FREE_HOOK_CALLED.load(core::sync::atomic::Ordering::SeqCst),
            1
        );

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

#[inline(never)]
unsafe fn do_c_shim_leak_alloc() -> *mut c_void {
    malloc(core::hint::black_box(64))
}

#[test]
fn test_c_shim_leak_detector() {
    let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
    unsafe {
        mnemosyne_reset_profiler_for_testing();
        mnemosyne_enable_leak_detector();
        assert_eq!(mnemosyne_is_leak_detector_enabled(), 1);

        let ptr = do_c_shim_leak_alloc();
        let ptr = core::hint::black_box(ptr);
        assert!(!ptr.is_null());

        mnemosyne_disable_leak_detector();
        assert_eq!(mnemosyne_is_leak_detector_enabled(), 0);

        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("mnemosyne_c_shim_leaks.txt");
        let path_str = path
            .to_str()
            .expect("temporary C-shim leak-report path must be valid UTF-8");

        // Convert string to C string
        let c_path = std::ffi::CString::new(path_str)
            .expect("temporary C-shim leak-report path must not contain interior NUL bytes");
        let count = mnemosyne_dump_leaks(c_path.as_ptr());
        assert_eq!(
            count, 1,
            "Expected exactly 1 leak captured in C shim (got {})",
            count
        );

        let content = std::fs::read_to_string(&path).expect("failed to read leak report");
        assert!(
            content.contains("do_c_shim_leak_alloc"),
            "Stack trace missing c_shim test function symbol: {}",
            content
        );

        let _ = std::fs::remove_file(&path);
        free(core::hint::black_box(ptr));
        mnemosyne_reset_profiler_for_testing();
    }
}

// ---------------------------------------------------------------------------
// Adversarial / hostile-input hardening for the C ABI surface.
//
// The repo mandates that every FFI surface taking hostile input be panic-free
// and never let a size/alignment argument drive UB or an unbounded allocation.
// These tests pin the boundary contracts the happy-path tests above do not:
// invalid alignments, allocator-unsupportable alignments (> SEGMENT_SIZE),
// zero size, shrink byte-preservation, extreme sizes, and a deterministic
// sweep that asserts "null or valid+aligned+writable+freeable" across a grid of
// hostile `(size, alignment)` pairs.
// ---------------------------------------------------------------------------

/// An alignment larger than the allocator's segment ceiling (2 MiB). A valid
/// power of two, so it passes the shim's own checks, but `thread_alloc` rejects
/// it — the boundary must surface that as a clean null, never UB.
const OVER_CEILING_ALIGN: usize = 4 * 1024 * 1024;

#[test]
fn aligned_alloc_rejects_zero_and_non_power_of_two_alignment() {
    let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
    // Alignment 0 is not a power of two.
    let a = unsafe { aligned_alloc(core::hint::black_box(0), core::hint::black_box(64)) };
    assert!(a.is_null(), "aligned_alloc(0, _) must return null");
    // 48 is a multiple-friendly but non-power-of-two alignment.
    let b = unsafe { aligned_alloc(core::hint::black_box(48), core::hint::black_box(96)) };
    assert!(
        b.is_null(),
        "aligned_alloc(non-power-of-two, _) must return null"
    );
}

#[test]
fn aligned_alloc_oversized_alignment_returns_null_without_ub() {
    let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
    // size is a multiple of the alignment, so only the > SEGMENT_SIZE ceiling
    // can reject it. The shim must surface that as null, not crash.
    let p = unsafe {
        aligned_alloc(
            core::hint::black_box(OVER_CEILING_ALIGN),
            core::hint::black_box(OVER_CEILING_ALIGN),
        )
    };
    assert!(
        p.is_null(),
        "aligned_alloc with alignment > SEGMENT_SIZE must return null"
    );
}

#[test]
fn aligned_alloc_zero_size_is_null_or_aligned_and_freeable() {
    let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
    // C11: `0` is a multiple of every alignment, so this is a well-formed
    // request. The shim substitutes `request = alignment`; the result is either
    // null (OOM) or a uniquely-freeable, correctly-aligned pointer.
    let p = unsafe { aligned_alloc(core::hint::black_box(64), core::hint::black_box(0)) };
    if !p.is_null() {
        assert_eq!(p as usize % 64, 0, "aligned_alloc(64, 0) not 64-aligned");
        unsafe { free(p) };
    }
}

#[test]
fn realloc_preserves_bytes_across_shrink() {
    let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
    let ptr = unsafe { malloc(4096) } as *mut u8;
    assert!(!ptr.is_null());
    for i in 0..64usize {
        unsafe { ptr.add(i).write((i as u8).wrapping_add(0x20)) };
    }
    let shrunk = unsafe { realloc(ptr as *mut c_void, 32) } as *mut u8;
    assert!(!shrunk.is_null());
    // C realloc preserves `min(old_usable, new_size)` bytes; on shrink to 32
    // that is the first 32 bytes.
    for i in 0..32usize {
        assert_eq!(
            unsafe { shrunk.add(i).read() },
            (i as u8).wrapping_add(0x20),
            "realloc shrink did not preserve byte {i}"
        );
    }
    unsafe { free(shrunk as *mut c_void) };
}

#[test]
fn posix_memalign_rejects_null_memptr() {
    let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
    let rc = unsafe { posix_memalign(core::ptr::null_mut(), 64, 64) };
    assert_eq!(rc, EINVAL, "posix_memalign(null memptr) must return EINVAL");
}

#[test]
fn posix_memalign_rejects_non_power_of_two_alignment() {
    let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
    let mut out: *mut c_void = core::ptr::null_mut();
    // 48 >= size_of::<*mut c_void>() but is not a power of two.
    let rc = unsafe { posix_memalign(&mut out as *mut *mut c_void, 48, 64) };
    assert_eq!(rc, EINVAL, "posix_memalign(non-pow2 align) must be EINVAL");
    assert!(out.is_null(), "memptr must be untouched on EINVAL");
}

#[test]
fn posix_memalign_oversized_alignment_fails_without_ub() {
    let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
    let mut out: *mut c_void = core::ptr::null_mut();
    // A valid (power-of-two, >= pointer size) alignment the allocator cannot
    // satisfy (> SEGMENT_SIZE): must fail with ENOMEM and leave `out` untouched,
    // never UB.
    let rc = unsafe { posix_memalign(&mut out as *mut *mut c_void, OVER_CEILING_ALIGN, 64) };
    assert_eq!(
        rc, ENOMEM,
        "posix_memalign with unsupportable alignment must return ENOMEM"
    );
    assert!(out.is_null(), "memptr must be untouched on failure");
}

#[test]
fn malloc_extreme_sizes_return_null_without_ub() {
    let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
    for size in [usize::MAX, (isize::MAX as usize) + 1, isize::MAX as usize] {
        let p = unsafe { malloc(core::hint::black_box(size)) };
        assert!(p.is_null(), "malloc({size}) must return null, not over-allocate");
    }
}

#[test]
fn calloc_overflow_pairs_return_null() {
    let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
    for (n, s) in [
        (usize::MAX, usize::MAX),
        (2, usize::MAX),
        (usize::MAX, 2),
        (1 << 33, 1 << 33),
    ] {
        let p = unsafe { calloc(core::hint::black_box(n), core::hint::black_box(s)) };
        assert!(p.is_null(), "calloc({n}, {s}) overflow must return null");
    }
}

#[test]
fn ffi_sweep_is_null_or_valid_aligned_writable_freeable() {
    let _guard = SHIM_LOCK.lock().expect("shim test lock poisoned");
    // Deterministic mini-fuzz: every (size, alignment) pair must yield either a
    // clean null or a pointer that is correctly aligned, writable across the
    // request, and freeable. Alignments stay <= 4 KiB so the sweep is cheap
    // (the > SEGMENT_SIZE ceiling is covered by the dedicated tests above).
    let sizes = [0usize, 1, 7, 8, 15, 64, 4096, 65537];
    let aligns = [8usize, 16, 64, 256, 4096];
    for &size in &sizes {
        // malloc: null, or writable-and-freeable.
        let m = unsafe { malloc(core::hint::black_box(size)) } as *mut u8;
        if !m.is_null() {
            if size > 0 {
                unsafe {
                    m.write(0xAB);
                    m.add(size - 1).write(0xCD);
                }
            }
            unsafe { free(m as *mut c_void) };
        }

        for &align in &aligns {
            // posix_memalign: rc==0 => out aligned+writable+freeable; rc!=0 =>
            // out untouched (null).
            let mut out: *mut c_void = core::ptr::null_mut();
            let rc = unsafe { posix_memalign(&mut out as *mut *mut c_void, align, size) };
            if rc == 0 {
                assert!(!out.is_null(), "posix_memalign rc==0 but null out");
                assert_eq!(
                    out as usize % align,
                    0,
                    "posix_memalign({align}, {size}) misaligned"
                );
                let b = out as *mut u8;
                if size > 0 {
                    unsafe {
                        b.write(0x5A);
                        b.add(size - 1).write(0xA5);
                    }
                }
                unsafe { free(out) };
            } else {
                assert!(out.is_null(), "posix_memalign failure left out non-null");
            }
        }
    }
}
