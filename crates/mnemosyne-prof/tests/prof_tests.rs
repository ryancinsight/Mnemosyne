use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne::{
    disable_profiling, dump_profile, enable_profiling, register_alloc_hook, register_free_hook,
    MnemosyneHeap, StandardPolicy,
};
use mnemosyne_backend::MemoryBackendWrapper as Backend;
use mnemosyne_local::{reset_options_for_testing, thread_alloc, thread_free};
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static FREE_COUNT: AtomicUsize = AtomicUsize::new(0);

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
    let _guard = TEST_LOCK.lock().unwrap();
    reset_options_for_testing();
    ALLOC_COUNT.store(0, Ordering::SeqCst);
    FREE_COUNT.store(0, Ordering::SeqCst);

    register_alloc_hook(Some(custom_alloc_hook));
    register_free_hook(Some(custom_free_hook));

    // Test global allocation
    unsafe {
        let ptr = thread_alloc::<StandardPolicy, Backend>(32, 16);
        assert!(!ptr.is_null());
        assert_eq!(ALLOC_COUNT.load(Ordering::SeqCst), 1);

        thread_free::<StandardPolicy, Backend>(ptr);
        assert_eq!(FREE_COUNT.load(Ordering::SeqCst), 1);
    }

    // Test heap allocation
    let heap = MnemosyneHeap::<StandardPolicy, Backend>::new();
    let layout = std::alloc::Layout::from_size_align(64, 8).unwrap();
    let ptr2 = heap.alloc(layout);
    assert!(!ptr2.is_null());
    assert_eq!(ALLOC_COUNT.load(Ordering::SeqCst), 2);

    unsafe {
        heap.free(ptr2);
    }
    assert_eq!(FREE_COUNT.load(Ordering::SeqCst), 2);

    // Unregister hooks
    register_alloc_hook(None);
    register_free_hook(None);
}

#[test]
#[inline(never)]
fn test_poisson_sampler_and_dump_profile() {
    let _guard = TEST_LOCK.lock().unwrap();
    reset_options_for_testing();

    // Enable profiling with a small interval (e.g. 100 bytes) so we get samples quickly
    enable_profiling(100);

    // Do some allocations
    let mut ptrs = Vec::new();
    for _ in 0..10 {
        unsafe {
            let ptr = thread_alloc::<StandardPolicy, Backend>(50, 8);
            assert!(!ptr.is_null());
            ptrs.push(ptr);
        }
    }

    // Dump profile
    let temp_dir = std::env::temp_dir();
    let prof_path = temp_dir.join("mnemosyne_test.prof");
    let prof_path_str = prof_path.to_str().unwrap();

    let res = dump_profile(prof_path_str);
    assert!(res.is_ok(), "dump_profile failed: {:?}", res.err());

    // Read the dumped profile
    let content = std::fs::read_to_string(prof_path_str).expect("Failed to read profile file");
    println!("Dumped Profile Content:\n{}", content);

    // Verify it contains entries (should contain symbol stacks) and size counts
    assert!(!content.is_empty(), "Profile should not be empty");
    assert!(
        content.contains("test_poisson_sampler_and_dump_profile"),
        "Profile must contain current test function name in stack"
    );

    // Clean up allocations
    for ptr in ptrs {
        unsafe {
            thread_free::<StandardPolicy, Backend>(ptr);
        }
    }

    disable_profiling();
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
    let _guard = TEST_LOCK.lock().unwrap();
    reset_options_for_testing();
    RECURSIVE_ALLOC_COUNT.store(0, Ordering::SeqCst);

    register_alloc_hook(Some(reentrant_alloc_hook));

    unsafe {
        let ptr = thread_alloc::<StandardPolicy, Backend>(32, 16);
        assert!(!ptr.is_null());
        thread_free::<StandardPolicy, Backend>(ptr);
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

    register_alloc_hook(None);
}
