use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne::{
    StandardPolicy, disable_profiling, dump_profile, enable_profiling, register_alloc_hook,
    register_free_hook,
};
use mnemosyne_backend::MemoryBackendWrapper as Backend;
use mnemosyne_heap::scope;
use mnemosyne_local::{reset_options_for_testing, thread_alloc, thread_free};
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static FREE_COUNT: AtomicUsize = AtomicUsize::new(0);

struct ProfilerResetGuard;

impl ProfilerResetGuard {
    fn new() -> Self {
        reset_options_for_testing();
        mnemosyne_prof::reset_profiler_for_testing();
        Self
    }
}

impl Drop for ProfilerResetGuard {
    fn drop(&mut self) {
        register_alloc_hook(None);
        register_free_hook(None);
        disable_profiling();
        mnemosyne_prof::disable_leak_detector();
        mnemosyne_prof::reset_profiler_for_testing();
    }
}

struct ThreadAllocation {
    ptr: *mut u8,
}

impl ThreadAllocation {
    unsafe fn new(size: usize, align: usize) -> Self {
        let ptr = unsafe { thread_alloc::<StandardPolicy, Backend>(size, align) };
        assert!(
            !ptr.is_null(),
            "thread allocation returned null for size {size} align {align}"
        );
        Self { ptr }
    }

    unsafe fn free_now(mut self) {
        let ptr = core::mem::replace(&mut self.ptr, core::ptr::null_mut());
        if !ptr.is_null() {
            unsafe { thread_free::<StandardPolicy, Backend>(ptr) };
        }
    }
}

impl Drop for ThreadAllocation {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { thread_free::<StandardPolicy, Backend>(self.ptr) };
            self.ptr = core::ptr::null_mut();
        }
    }
}

unsafe extern "C" fn custom_alloc_hook(ptr: *mut core::ffi::c_void, size: usize) {
    if !ptr.is_null() && size > 0 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

unsafe extern "C" fn custom_free_hook(ptr: *mut core::ffi::c_void, size: usize) {
    if !ptr.is_null() && size > 0 {
        FREE_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn test_custom_trace_hooks() {
    let _guard = TEST_LOCK
        .lock()
        .expect("profiler integration test lock was poisoned");
    let _profiler_guard = ProfilerResetGuard::new();
    ALLOC_COUNT.store(0, Ordering::SeqCst);
    FREE_COUNT.store(0, Ordering::SeqCst);

    register_alloc_hook(Some(custom_alloc_hook));
    register_free_hook(Some(custom_free_hook));

    // Test global allocation
    let ptr = unsafe { ThreadAllocation::new(32, 16) };
    assert_eq!(ALLOC_COUNT.load(Ordering::SeqCst), 1);
    unsafe { ptr.free_now() };
    assert_eq!(FREE_COUNT.load(Ordering::SeqCst), 1);

    // Test heap allocation
    let layout = std::alloc::Layout::from_size_align(64, 8)
        .expect("64-byte allocation with 8-byte alignment is a valid Layout");
    scope::<StandardPolicy, Backend, _, _>(|heap, mut token| {
        let block = heap
            .alloc(&token, layout)
            .expect("profiled heap allocation failed");
        assert_eq!(ALLOC_COUNT.load(Ordering::SeqCst), 2);
        heap.free_uninit(&mut token, block);
    });
    assert_eq!(FREE_COUNT.load(Ordering::SeqCst), 2);
}

#[test]
#[inline(never)]
fn test_poisson_sampler_and_dump_profile() {
    let _guard = TEST_LOCK
        .lock()
        .expect("profiler integration test lock was poisoned");
    let _profiler_guard = ProfilerResetGuard::new();

    // Enable profiling with a small interval (e.g. 100 bytes) so we get samples quickly
    enable_profiling(100);

    // Do some allocations
    let mut ptrs = Vec::new();
    for _ in 0..10 {
        ptrs.push(unsafe { ThreadAllocation::new(50, 8) });
    }

    // Dump profile
    let temp_dir = std::env::temp_dir();
    let prof_path = temp_dir.join("mnemosyne_test.prof");
    let prof_path_str = prof_path
        .to_str()
        .expect("temporary profile path must be valid UTF-8");

    let res = dump_profile(prof_path_str);
    assert!(res.is_ok(), "dump_profile failed: {:?}", res.err());

    // Read the dumped profile
    let content =
        std::fs::read_to_string(prof_path_str).expect("failed to read profiler output file");
    println!("Dumped Profile Content:\n{}", content);

    // Verify it contains entries (should contain symbol stacks) and size counts
    assert!(!content.is_empty(), "Profile should not be empty");
    assert!(
        content.contains("test_poisson_sampler_and_dump_profile"),
        "Profile must contain current test function name in stack"
    );

    let _ = std::fs::remove_file(prof_path);
}

static RECURSIVE_ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn reentrant_alloc_hook(ptr: *mut core::ffi::c_void, size: usize) {
    if !ptr.is_null() && size > 0 {
        RECURSIVE_ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        // Trigger a recursive allocation inside the hook!
        unsafe {
            let nested = thread_alloc::<StandardPolicy, Backend>(16, 8);
            if !nested.is_null() {
                thread_free::<StandardPolicy, Backend>(nested);
            }
        }
    }
}

#[test]
fn test_reentrancy_protection() {
    let _guard = TEST_LOCK
        .lock()
        .expect("profiler integration test lock was poisoned");
    let _profiler_guard = ProfilerResetGuard::new();
    RECURSIVE_ALLOC_COUNT.store(0, Ordering::SeqCst);

    register_alloc_hook(Some(reentrant_alloc_hook));

    unsafe {
        let ptr = ThreadAllocation::new(32, 16);
        ptr.free_now();
    }

    // The hook should be called for the primary allocation.
    // The nested allocation inside the hook should be executed but its hook execution must be bypassed
    // by the re-entrancy guard to avoid infinite recursion / stack overflow.
    // Therefore, RECURSIVE_ALLOC_COUNT should be exactly 1!
    assert_eq!(
        RECURSIVE_ALLOC_COUNT.load(Ordering::SeqCst),
        1,
        "Re-entrancy guard failed to prevent recursive hook execution"
    );
}

#[inline(never)]
unsafe fn do_leak_alloc() -> ThreadAllocation {
    unsafe { ThreadAllocation::new(128, 8) }
}

#[test]
fn test_leak_detector_and_dump_leaks() {
    let _guard = TEST_LOCK
        .lock()
        .expect("profiler integration test lock was poisoned");
    let _profiler_guard = ProfilerResetGuard::new();

    // Enable the leak detector
    mnemosyne_prof::enable_leak_detector();
    assert!(mnemosyne_prof::is_leak_detector_enabled());

    // Do some allocations
    let ptr1 = unsafe { ThreadAllocation::new(64, 8) };
    let _ptr2 = unsafe { do_leak_alloc() };

    // Free one allocation, keep the other as a leak
    unsafe { ptr1.free_now() };

    // Dump leaks
    let temp_dir = std::env::temp_dir();
    let leak_path = temp_dir.join("mnemosyne_leaks_test.txt");
    let leak_path_str = leak_path
        .to_str()
        .expect("temporary leak-report path must be valid UTF-8");

    let leak_count_res = mnemosyne_prof::dump_leaks(leak_path_str);
    assert!(
        leak_count_res.is_ok(),
        "dump_leaks failed: {:?}",
        leak_count_res.err()
    );
    let leak_count = leak_count_res.expect("dump_leaks failed after positive status check");

    // Verify report details
    assert_eq!(
        leak_count, 1,
        "expected exactly 1 leak, found {}",
        leak_count
    );

    let content =
        std::fs::read_to_string(leak_path_str).expect("failed to read leak report output file");
    println!("Leak Report Content:\n{}", content);

    assert!(
        content.contains("Mnemosyne Leak Report:"),
        "Report header missing"
    );
    assert!(
        content.contains("Leak of 128 bytes"),
        "Leak size missing or incorrect"
    );
    assert!(
        content.contains("do_leak_alloc"),
        "Stack trace missing the test function symbol: {}",
        content
    );

    let _ = std::fs::remove_file(leak_path);
}
