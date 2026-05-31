use core::alloc::Layout;
use mnemosyne_core::StandardPolicy;
use mnemosyne_heap::MnemosyneHeap;

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_multi_heap_basic() {
    let _guard = TEST_LOCK.lock().unwrap();
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
    let _guard = TEST_LOCK.lock().unwrap();
    use std::sync::{Arc, Mutex};
    use std::thread;
    let heap = Arc::new(Mutex::new(MnemosyneHeap::<StandardPolicy>::new()));

    let heap_clone = heap.clone();
    let layout = Layout::from_size_align(64, 8).unwrap();

    let handle = thread::spawn(move || {
        let heap_guard = heap_clone.lock().unwrap();
        let ptr = heap_guard.alloc(layout);
        assert!(!ptr.is_null());
        unsafe { ptr.write(42) };
        ptr as usize
    });

    let ptr_val = handle.join().unwrap();
    let ptr = ptr_val as *mut u8;

    // Free the pointer on the main thread
    unsafe {
        heap.lock().unwrap().free(ptr);
    }
}

#[test]
fn test_runtime_options_override_default_retention() {
    let _guard = TEST_LOCK.lock().unwrap();
    use mnemosyne_arena::HasSegmentPool;
    use mnemosyne_backend::MemoryBackendWrapper;

    // Reset options to default
    mnemosyne_local::reset_options_for_testing();

    // 1. Force the option to 0 via env var
    std::env::set_var("MNEMOSYNE_MAX_RETAINED_SEGMENTS", "0");

    let pool = <MemoryBackendWrapper as HasSegmentPool>::global_segment_pool();
    let initial_retained = pool.retained_count();

    // Do an allocation via MnemosyneHeap to trigger options parsing and then drop it
    {
        let heap = MnemosyneHeap::<StandardPolicy>::new();
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
