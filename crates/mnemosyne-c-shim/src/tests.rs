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
