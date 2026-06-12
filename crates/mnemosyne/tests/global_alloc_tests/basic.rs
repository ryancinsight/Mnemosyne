use super::*;

#[test]
fn test_basic_allocation() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let x = std::boxed::Box::new(42);
    assert_eq!(*x, 42);
    drop(x);
}

#[test]
fn test_multithreaded_allocation() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let mut handles = std::vec::Vec::new();
    for _ in 0..10 {
        handles.push(thread::spawn(|| {
            for _ in 0..100 {
                let mut v = std::vec::Vec::new();
                for i in 0..100 {
                    v.push(i);
                }
                assert_eq!(v[50], 50);
            }
        }));
    }
    for handle in handles {
        handle.join().expect("allocation worker thread panicked");
    }
}

#[test]
fn test_overflow_protection() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    // 1. Direct call to thread_alloc with size that triggers overflow
    let ptr1 = unsafe {
        mnemosyne_local::thread_alloc::<StandardPolicy, mnemosyne_backend::MemoryBackendWrapper>(
            usize::MAX - 8,
            8,
        )
    };
    assert!(
        ptr1.is_null(),
        "Allocation should fail and return null on overflow"
    );

    // 2. Request a layout of isize::MAX (largest valid layout size) which will fail OS allocation
    let layout = Layout::from_size_align(isize::MAX as usize - 7, 8)
        .expect("isize::MAX - 7 with 8-byte alignment is a valid Layout");
    let ptr2 = unsafe { ALLOCATOR.alloc(layout) };
    assert!(
        ptr2.is_null(),
        "OS allocation should fail and return null for isize::MAX"
    );
}

#[test]
fn test_zero_size_allocation_returns_null() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let layout = Layout::from_size_align(0, 8).expect("zero-size 8-byte aligned Layout is valid");

    let ptr = unsafe { ALLOCATOR.alloc(layout) };
    assert!(ptr.is_null(), "zero-size Mnemosyne alloc returned {ptr:?}");

    let allocator = MnemosyneAllocator::<StandardPolicy>::new();
    let generic_ptr = unsafe { allocator.alloc(layout) };
    assert!(
        generic_ptr.is_null(),
        "zero-size generic allocator returned {generic_ptr:?}"
    );
}
