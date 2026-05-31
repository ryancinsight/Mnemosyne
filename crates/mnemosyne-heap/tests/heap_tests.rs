use core::alloc::Layout;
use mnemosyne_core::StandardPolicy;
use mnemosyne_heap::MnemosyneHeap;

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_multi_heap_basic() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let heap = MnemosyneHeap::<StandardPolicy>::new();
    let layout = Layout::from_size_align(32, 8).unwrap();
    let ptr = heap.alloc(layout);
    assert!(!ptr.is_null());
    unsafe { ptr.write(123) };
    assert_eq!(unsafe { ptr.read() }, 123);

    // Test realloc
    let ptr2 = unsafe { heap.realloc(ptr, layout, 64) };
    assert!(!ptr2.is_null());
    assert_eq!(unsafe { ptr2.read() }, 123);

    unsafe {
        heap.free(ptr2);
    }
}

#[test]
fn test_multi_heap_cross_thread() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use std::sync::{Arc, Mutex};
    use std::thread;
    let heap = Arc::new(Mutex::new(MnemosyneHeap::<StandardPolicy>::new()));

    let heap_clone = heap.clone();
    let layout = Layout::from_size_align(64, 8).unwrap();

    let handle = thread::spawn(move || {
        let heap_guard = heap_clone.lock().unwrap_or_else(|e| e.into_inner());
        let ptr = heap_guard.alloc(layout);
        assert!(!ptr.is_null());
        unsafe { ptr.write(42) };
        ptr as usize
    });

    let ptr_val = handle.join().unwrap();
    let ptr = ptr_val as *mut u8;

    // Free the pointer on the main thread
    unsafe {
        heap.lock().unwrap_or_else(|e| e.into_inner()).free(ptr);
    }
}

#[test]
fn test_runtime_options_override_default_retention() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use mnemosyne_arena::HasSegmentPool;
    use mnemosyne_backend::MemoryBackendWrapper;

    // Reset options to default
    mnemosyne_local::reset_options_for_testing();

    // 1. Force the option to 0 via env var
    std::env::set_var("MNEMOSYNE_MAX_RETAINED_SEGMENTS", "0");

    let pool = <MemoryBackendWrapper as HasSegmentPool>::global_segment_pool();
    unsafe {
        mnemosyne_arena::purge_segment_pool::<MemoryBackendWrapper>();
    }
    let initial_retained = pool.retained_count();

    // Do an allocation via MnemosyneHeap to trigger options parsing and then drop it
    {
        let heap = MnemosyneHeap::<StandardPolicy, MemoryBackendWrapper>::new();
        let layout = Layout::from_size_align(32, 8).unwrap();
        let ptr = heap.alloc(layout);
        assert!(!ptr.is_null());
        unsafe {
            heap.free(ptr);
        }
    }

    let final_retained = pool.retained_count();
    // Since MAX_RETAINED_SEGMENTS is 0, the segment from the heap should have been deallocated
    // and not pushed into the pool, so retained count must stay equal to the initial_retained.
    assert_eq!(final_retained, initial_retained);

    // Reset options again to defaults
    mnemosyne_local::reset_options_for_testing();
    std::env::remove_var("MNEMOSYNE_MAX_RETAINED_SEGMENTS");
}

#[test]
fn multi_heap_isolates_allocation_streams() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let heap1 = MnemosyneHeap::<StandardPolicy>::new();
    let heap2 = MnemosyneHeap::<StandardPolicy>::new();

    let layout = Layout::from_size_align(32, 8).unwrap();

    // Allocate from both heaps
    let ptr1 = heap1.alloc(layout);
    let ptr2 = heap2.alloc(layout);

    assert!(!ptr1.is_null());
    assert!(!ptr2.is_null());
    assert_ne!(ptr1, ptr2);

    unsafe {
        ptr1.write(111);
        ptr2.write(222);

        assert_eq!(ptr1.read(), 111);
        assert_eq!(ptr2.read(), 222);

        heap1.free(ptr1);
        heap2.free(ptr2);
    }
}

#[test]
fn multi_heap_release_does_not_touch_other_heaps() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use mnemosyne_arena::HasSegmentPool;
    use mnemosyne_backend::MemoryBackendWrapper;

    // Reset options for testing
    mnemosyne_local::reset_options_for_testing();
    // Allow up to 10 retained segments so dropping the heap caches its segment
    mnemosyne_core::options::set_options(mnemosyne_core::options::MnemosyneOptions {
        max_retained_segments: 10,
        purge_cadence_ms: 0,
        enable_hugepage_hint: true,
    });
    mnemosyne_local::mark_options_initialized();

    let pool = <MemoryBackendWrapper as HasSegmentPool>::global_segment_pool();
    unsafe {
        mnemosyne_arena::purge_segment_pool::<MemoryBackendWrapper>();
    }
    let initial_retained = pool.retained_count();

    // 1. Create two separate heaps using MemoryBackendWrapper
    let heap1 = MnemosyneHeap::<StandardPolicy, MemoryBackendWrapper>::new();
    let heap2 = MnemosyneHeap::<StandardPolicy, MemoryBackendWrapper>::new();

    let layout = Layout::from_size_align(32, 8).unwrap();

    // 2. Allocate blocks
    let ptr1 = heap1.alloc(layout);
    let ptr2 = heap2.alloc(layout);

    assert!(!ptr1.is_null());
    assert!(!ptr2.is_null());

    unsafe {
        ptr1.write(55);
        ptr2.write(77);
    }

    // 3. Free ptr1 in heap1, then drop heap1
    unsafe {
        heap1.free(ptr1);
    }
    drop(heap1);

    // Dropping heap1 with 0 live allocations causes its segment to be returned to the global pool.
    // Retained segments should increase.
    let retained_after_heap1 = pool.retained_count();
    assert!(
        retained_after_heap1 > initial_retained,
        "Segment from heap1 should have been returned to global pool"
    );

    // 4. Verify heap2 is completely untouched and ptr2 is still valid
    assert_eq!(unsafe { ptr2.read() }, 77);

    // Clean up heap2
    unsafe {
        heap2.free(ptr2);
    }
    drop(heap2);

    mnemosyne_local::reset_options_for_testing();
}

#[test]
fn test_programmatic_options_configure() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use mnemosyne_arena::HasSegmentPool;
    use mnemosyne_backend::MemoryBackendWrapper;

    mnemosyne_local::reset_options_for_testing();

    // Retrieve default options
    let default_options = mnemosyne_core::options::get_options();
    assert_eq!(default_options.max_retained_segments, 1024);

    // Set retained segment limit to 0
    mnemosyne_core::options::set_options(mnemosyne_core::options::MnemosyneOptions {
        max_retained_segments: 0,
        ..default_options
    });
    mnemosyne_local::mark_options_initialized();

    let active_options = mnemosyne_core::options::get_options();
    assert_eq!(active_options.max_retained_segments, 0);

    // Allocate and free to verify segment retention is bounded by 0
    let pool = <MemoryBackendWrapper as HasSegmentPool>::global_segment_pool();
    unsafe {
        mnemosyne_arena::purge_segment_pool::<MemoryBackendWrapper>();
    }
    let initial_retained = pool.retained_count();

    {
        let heap = MnemosyneHeap::<StandardPolicy, MemoryBackendWrapper>::new();
        let layout = Layout::from_size_align(32, 8).unwrap();
        let ptr = heap.alloc(layout);
        assert!(!ptr.is_null());
        unsafe {
            heap.free(ptr);
        }
    }

    let final_retained = pool.retained_count();
    assert_eq!(final_retained, initial_retained);

    // Reset options
    mnemosyne_local::reset_options_for_testing();
}
