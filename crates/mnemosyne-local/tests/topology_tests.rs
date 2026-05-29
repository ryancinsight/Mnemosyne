use mnemosyne_local::MnemosyneHeap;
use mnemosyne_core::StandardPolicy;
use core::alloc::Layout;

#[test]
fn test_multi_heap_basic() {
    let heap = MnemosyneHeap::<StandardPolicy>::new();
    let layout = Layout::from_size_align(32, 8).unwrap();
    let ptr = heap.alloc(layout);
    assert!(!ptr.is_null());
    unsafe { ptr.write(123) };
    assert_eq!(unsafe { ptr.read() }, 123);
    
    // Test realloc
    let ptr2 = heap.realloc(ptr, layout, 64);
    assert!(!ptr2.is_null());
    assert_eq!(unsafe { ptr2.read() }, 123);
    
    heap.free(ptr2);
}

#[test]
fn test_multi_heap_cross_thread() {
    use std::thread;
    use std::sync::{Arc, Mutex};
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
    heap.lock().unwrap().free(ptr);
}

#[test]
fn test_per_cpu_cache() {
    use mnemosyne_local::per_cpu;
    per_cpu::PER_CPU_CACHE_ENABLED.store(true, core::sync::atomic::Ordering::Relaxed);
    let layout = Layout::from_size_align(16, 8).unwrap();
    // Allocate a block via the global allocator or a heap
    let heap = MnemosyneHeap::<StandardPolicy>::new();
    let ptr = heap.alloc(layout);
    assert!(!ptr.is_null());
    
    // Try to free to the CPU cache
    let freed = per_cpu::try_free_cpu::<StandardPolicy>(ptr, 0);
    assert!(freed);
    
    // Pop it back from the CPU cache
    let popped = per_cpu::try_alloc_cpu::<StandardPolicy>(0);
    assert_eq!(popped, ptr);
    
    heap.free(popped);
    per_cpu::PER_CPU_CACHE_ENABLED.store(false, core::sync::atomic::Ordering::Relaxed);
}

#[test]
fn test_numa_node_segment_retention() {
    use mnemosyne_arena::current_numa_node;
    let node = current_numa_node();
    println!("Current NUMA node: {}", node);
    
    // Verify segment allocation sets numa_node
    unsafe {
        let segment = mnemosyne_arena::allocate_segment::<mnemosyne_backend::MemoryBackendWrapper>().unwrap();
        assert_eq!((*segment).numa_node, node);
        mnemosyne_arena::deallocate_segment::<mnemosyne_backend::MemoryBackendWrapper>(segment);
    }
}
