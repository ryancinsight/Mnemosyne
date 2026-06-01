use core::alloc::{GlobalAlloc, Layout};
use std::thread;

use mnemosyne::{
    enable_leak_detector, disable_leak_detector, is_leak_detector_enabled, dump_leaks,
    is_cuda_available, CudaUnifiedBackend,
    memory_stats, memory_stats_generic, purge, reset,
    Mnemosyne, MnemosyneAllocator, StandardPolicy, SecurePolicy,
    usable_size,
};

#[global_allocator]
static ALLOCATOR: Mnemosyne = Mnemosyne;

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    let layout =
        Layout::from_size_align(0, 8).expect("zero-size 8-byte aligned Layout is valid");

    let ptr = unsafe { ALLOCATOR.alloc(layout) };
    assert!(ptr.is_null(), "zero-size Mnemosyne alloc returned {ptr:?}");

    let allocator = MnemosyneAllocator::<StandardPolicy>::new();
    let generic_ptr = unsafe { allocator.alloc(layout) };
    assert!(
        generic_ptr.is_null(),
        "zero-size generic allocator returned {generic_ptr:?}"
    );
}

#[test]
fn realloc_within_usable_size_returns_same_pointer_and_preserves_bytes() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let old_layout =
        Layout::from_size_align(24, 8).expect("24-byte 8-byte aligned Layout is valid");
    let ptr = unsafe { ALLOCATOR.alloc(old_layout) };
    assert!(!ptr.is_null(), "realloc setup allocation failed");
    unsafe {
        core::ptr::write_bytes(ptr, 0xA5, old_layout.size());
    }

    let usable = unsafe { usable_size(ptr) };
    assert!(
        usable >= 32,
        "test requires allocation usable size >= 32, got {usable}"
    );
    let new_ptr = unsafe { ALLOCATOR.realloc(ptr, old_layout, 32) };
    assert_eq!(
        new_ptr, ptr,
        "standard realloc within usable size should stay in place"
    );
    for offset in 0..old_layout.size() {
        let byte = unsafe { *new_ptr.add(offset) };
        assert_eq!(byte, 0xA5, "realloc failed to preserve byte {offset}");
    }

    let new_layout =
        Layout::from_size_align(32, 8).expect("32-byte 8-byte aligned Layout is valid");
    unsafe { ALLOCATOR.dealloc(new_ptr, new_layout) };
}

#[test]
fn secure_realloc_within_usable_size_uses_replacement_allocation() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let allocator = MnemosyneAllocator::<SecurePolicy>::new();
    let old_layout =
        Layout::from_size_align(24, 8).expect("24-byte 8-byte aligned Layout is valid");
    let ptr = unsafe { allocator.alloc(old_layout) };
    assert!(!ptr.is_null(), "secure realloc setup allocation failed");
    unsafe {
        core::ptr::write_bytes(ptr, 0x5A, old_layout.size());
    }

    let new_ptr = unsafe { allocator.realloc(ptr, old_layout, 32) };
    assert!(
        !new_ptr.is_null(),
        "secure realloc returned null for in-class growth"
    );
    assert_ne!(
        new_ptr, ptr,
        "secure realloc must not grow in place without initializing new bytes"
    );
    for offset in 0..old_layout.size() {
        let byte = unsafe { *new_ptr.add(offset) };
        assert_eq!(
            byte, 0x5A,
            "secure realloc failed to preserve byte {offset}"
        );
    }
    for offset in old_layout.size()..32 {
        let byte = unsafe { *new_ptr.add(offset) };
        assert_eq!(byte, 0, "secure realloc failed to zero new byte {offset}");
    }

    let new_layout =
        Layout::from_size_align(32, 8).expect("32-byte 8-byte aligned Layout is valid");
    unsafe { allocator.dealloc(new_ptr, new_layout) };
}

#[test]
fn realloc_zero_size_returns_null_without_allocating() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let layout =
        Layout::from_size_align(24, 8).expect("24-byte 8-byte aligned Layout is valid");
    let ptr = unsafe { ALLOCATOR.alloc(layout) };
    assert!(!ptr.is_null(), "zero-size realloc setup allocation failed");
    let new_ptr = unsafe { ALLOCATOR.realloc(ptr, layout, 0) };
    assert!(
        new_ptr.is_null(),
        "zero-size realloc returned non-null pointer {new_ptr:?}"
    );

    let null_realloc = unsafe { ALLOCATOR.realloc(core::ptr::null_mut(), layout, 0) };
    assert!(
        null_realloc.is_null(),
        "null zero-size realloc returned non-null pointer {null_realloc:?}"
    );
}

#[test]
fn test_segment_reclamation() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    // Allocate and deallocate large blocks multiple times
    // If segments are not reclaimed/reused, this would exhaust virtual address space or leak memory.
    for _ in 0..20 {
        let mut allocations = std::vec::Vec::new();
        for _ in 0..10 {
            // Allocate 1MB blocks (large allocations)
            let layout = Layout::from_size_align(1024 * 1024, 8)
                .expect("1 MiB with 8-byte alignment is a valid Layout");
            let ptr = unsafe { ALLOCATOR.alloc(layout) };
            assert!(
                !ptr.is_null(),
                "1 MiB segment-reclamation allocation failed"
            );
            allocations.push((ptr, layout));
        }
        for (ptr, layout) in allocations {
            unsafe { ALLOCATOR.dealloc(ptr, layout) };
        }
    }
}

#[test]
fn test_memory_stats_retention_bound() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    const SIZES: [usize; 40] = [
        8, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96, 104, 112, 120, 128, 160, 192, 224, 256,
        288, 320, 352, 384, 416, 448, 480, 512, 640, 768, 896, 1024, 1152, 1280, 1408, 1536,
        1664, 1792, 1920, 2048,
    ];
    let empty_layout =
        Layout::from_size_align(8, 8).expect("8-byte size and alignment is a valid Layout");
    let mut allocations = [(core::ptr::null_mut(), empty_layout); SIZES.len()];
    let warmup = unsafe { ALLOCATOR.alloc(empty_layout) };
    assert!(!warmup.is_null(), "memory-stats warm-up allocation failed");
    unsafe { ALLOCATOR.dealloc(warmup, empty_layout) };
    let baseline_live_allocations = memory_stats().current_thread_live_allocations;

    for (index, size) in SIZES.into_iter().enumerate() {
        let layout = Layout::from_size_align(size, 8)
            .expect("test size table contains valid 8-byte aligned Layout sizes");
        let ptr = unsafe { ALLOCATOR.alloc(layout) };
        assert!(
            !ptr.is_null(),
            "memory-stats allocation failed for size {size}"
        );
        allocations[index] = (ptr, layout);
    }

    let after_alloc_live_allocations = memory_stats().current_thread_live_allocations;
    assert!(
        after_alloc_live_allocations >= baseline_live_allocations + SIZES.len(),
        "live allocation count did not increase by at least the test allocation count: baseline={} after_alloc={} test_allocations={}",
        baseline_live_allocations,
        after_alloc_live_allocations,
        SIZES.len()
    );

    for (ptr, layout) in allocations {
        unsafe { ALLOCATOR.dealloc(ptr, layout) };
    }

    let stats = memory_stats();
    assert!(
        stats.current_mapped_bytes <= stats.peak_mapped_bytes,
        "current_mapped_bytes ({}) exceeds peak_mapped_bytes ({})",
        stats.current_mapped_bytes,
        stats.peak_mapped_bytes
    );
    assert!(
        stats.retained_free_segments <= stats.max_retained_free_segments,
        "retained_free_segments ({}) exceeds bound ({})",
        stats.retained_free_segments,
        stats.max_retained_free_segments
    );
    assert!(
        stats.current_thread_live_allocations <= after_alloc_live_allocations - SIZES.len(),
        "live allocation count did not drop by the test allocation count: after_alloc={} after_free={} test_allocations={}",
        after_alloc_live_allocations,
        stats.current_thread_live_allocations,
        SIZES.len()
    );
    assert!(stats
        .size_class_occupancy
        .iter()
        .any(|occupancy| occupancy.active_pages > 0));
}

#[test]
fn test_purge() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    // Clear any existing segments in the pool.
    purge();

    let segment = unsafe {
        mnemosyne_arena::allocate_segment::<mnemosyne_backend::MemoryBackendWrapper>()
            .expect("segment allocation must succeed")
    };
    unsafe {
        mnemosyne_arena::deallocate_segment::<mnemosyne_backend::MemoryBackendWrapper>(segment);
    }

    // The segment is now in the global segment pool.
    let stats_before = memory_stats();
    assert!(
        stats_before.retained_free_segments > 0,
        "Expected at least one segment to be retained in the pool"
    );

    purge();

    let stats_after = memory_stats();
    assert_eq!(
        stats_after.retained_free_segments, 0,
        "Expected zero segments to be retained in the pool after purge"
    );
    assert!(
        stats_after.purged_segments > stats_before.purged_segments,
        "Expected purged_segments count to increase"
    );
    assert!(
        stats_after.purge_calls > stats_before.purge_calls,
        "Expected purge_calls count to increase"
    );
    assert!(
        stats_after.purged_bytes > stats_before.purged_bytes,
        "Expected purged_bytes count to increase"
    );
}

#[test]
fn test_reset_keeps_segments_cached_and_records_telemetry() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    // Start from a clean pool so the retention count is deterministic.
    purge();

    // Cache one segment via the standard alloc/dealloc round trip.
    let segment = unsafe {
        mnemosyne_arena::allocate_segment::<mnemosyne_backend::MemoryBackendWrapper>()
            .expect("segment allocation must succeed")
    };
    unsafe {
        mnemosyne_arena::deallocate_segment::<mnemosyne_backend::MemoryBackendWrapper>(segment);
    }
    let stats_before = memory_stats();
    assert!(
        stats_before.retained_free_segments >= 1,
        "expected at least one cached segment before reset"
    );

    reset();

    let stats_after = memory_stats();
    // Reset preserves the cache: retention count does not drop. The
    // process-wide retained pool may grow if another completed test
    // thread's TLS allocator returns an owned segment concurrently.
    assert!(
        stats_after.retained_free_segments >= stats_before.retained_free_segments,
        "reset must not evict retained segments: before={} after={}",
        stats_before.retained_free_segments,
        stats_after.retained_free_segments
    );
    // Reset always increments its own call counter, regardless of
    // whether the backend confirmed the page-reset advice.
    assert!(
        stats_after.reset_calls > stats_before.reset_calls,
        "reset_calls counter did not advance: before={} after={}",
        stats_before.reset_calls,
        stats_after.reset_calls
    );
    // On Windows the wrapper backend implements page_reset via
    // VirtualAlloc(MEM_RESET) which always succeeds for active
    // mappings, so reset_segments should also advance. On platforms
    // where the kernel declines the advice, this is permitted to
    // stay equal — the test asserts only the call counter advanced.
    assert!(
        stats_after.reset_segments >= stats_before.reset_segments,
        "reset_segments regressed: before={} after={}",
        stats_before.reset_segments,
        stats_after.reset_segments
    );
    // Purge counters are not perturbed by reset.
    assert_eq!(
        stats_after.purge_calls, stats_before.purge_calls,
        "reset must not increment purge_calls"
    );
    assert_eq!(
        stats_after.purged_segments, stats_before.purged_segments,
        "reset must not increment purged_segments"
    );

    // The address space remains writable through the cached mapping
    // — drain the pool to pop the segment and write through it.
    purge();
}

#[test]
fn test_realloc_within_class_returns_same_ptr() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    // 32 B request lands in size class 1 (block_size = 32 B); shrinking
    // and growing-within-class must both return the same pointer with
    // no copy-and-free.
    let layout = Layout::from_size_align(32, 8).expect("valid layout");
    let ptr = unsafe { ALLOCATOR.alloc(layout) };
    assert!(!ptr.is_null());

    // Mark a sentinel byte so we can detect any unintended copy.
    unsafe { ptr.write(0x5A) };

    // Shrink within class.
    let shrunk = unsafe { ALLOCATOR.realloc(ptr, layout, 16) };
    assert_eq!(
        shrunk, ptr,
        "shrink within class must return the same pointer"
    );

    // Grow within class.
    let grown = unsafe { ALLOCATOR.realloc(shrunk, layout, 32) };
    assert_eq!(grown, ptr, "grow within class must return the same pointer");

    // Confirm the sentinel survived — no copy happened.
    assert_eq!(
        unsafe { ptr.read() },
        0x5A,
        "sentinel byte mutated; an unwanted copy occurred"
    );

    unsafe { ALLOCATOR.dealloc(ptr, layout) };
}

#[test]
fn test_realloc_large_half_shrink_returns_same_ptr() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let old_layout = Layout::from_size_align(4 * 1024 * 1024, 8).expect("valid layout");
    let new_size = 2 * 1024 * 1024;
    let ptr = unsafe { ALLOCATOR.alloc(old_layout) };
    assert!(!ptr.is_null(), "large realloc setup allocation failed");

    unsafe {
        ptr.write(0xC3);
        ptr.add(new_size - 1).write(0x3C);
    }

    let shrunk = unsafe { ALLOCATOR.realloc(ptr, old_layout, new_size) };
    assert_eq!(
        shrunk, ptr,
        "standard half-shrink must avoid allocate-copy-free churn"
    );
    assert_eq!(unsafe { shrunk.read() }, 0xC3);
    assert_eq!(unsafe { shrunk.add(new_size - 1).read() }, 0x3C);

    let new_layout = Layout::from_size_align(new_size, 8).expect("valid layout");
    unsafe { ALLOCATOR.dealloc(shrunk, new_layout) };
}

#[test]
fn test_realloc_across_class_copies_and_returns_new_ptr() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    // 16 B request → class 0 (block_size 16). Growing to 64 B requires
    // a different size class; the realloc must allocate, copy, and
    // free. The original sentinel bytes must appear in the new
    // allocation.
    let small_layout = Layout::from_size_align(16, 8).expect("valid layout");
    let ptr = unsafe { ALLOCATOR.alloc(small_layout) };
    assert!(!ptr.is_null());

    // Fill the 16 B with a known pattern.
    for i in 0..16usize {
        unsafe { ptr.add(i).write((i as u8).wrapping_add(0xA0)) };
    }

    let new_ptr = unsafe { ALLOCATOR.realloc(ptr, small_layout, 64) };
    assert!(!new_ptr.is_null());

    // The new allocation may or may not coincide with `ptr` depending
    // on the size-class choice; what matters is that the prefix
    // bytes were preserved.
    for i in 0..16usize {
        assert_eq!(
            unsafe { new_ptr.add(i).read() },
            (i as u8).wrapping_add(0xA0),
            "realloc across class did not preserve byte {i}"
        );
    }

    let new_layout = Layout::from_size_align(64, 8).expect("valid layout");
    unsafe { ALLOCATOR.dealloc(new_ptr, new_layout) };
}

#[test]
fn test_realloc_does_not_copy_past_layout_size() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    // Pins the slow-path copy-length contract: even when the caller's
    // allocation has size-class slack (usable_size > layout.size), the
    // slow path must copy *only* layout.size bytes. If it instead
    // copied usable_size bytes, an accidental write in the slack
    // region would propagate to the new allocation.
    //
    // Setup: 8 B request lands in class 0 (block_size 16 B), so
    // layout.size = 8 but usable_size(ptr) = 16. Use SecurePolicy so
    // the replacement allocation has defined zero bytes beyond the
    // copied user region; this lets the test inspect [8, 16) without
    // reading uninitialized memory.
    let allocator = MnemosyneAllocator::<SecurePolicy>::new();
    let small_layout = Layout::from_size_align(8, 8).expect("valid layout");
    let ptr = unsafe { allocator.alloc(small_layout) };
    assert!(!ptr.is_null());
    // Sanity-check the slack window exists.
    let reported = unsafe { usable_size(ptr) };
    assert!(
        reported >= 16,
        "8 B request must land in a class with at least 16 B usable; got {reported}"
    );

    // User region: bytes 0..8.
    for i in 0..8usize {
        unsafe { ptr.add(i).write(0xAA) };
    }
    // Slack region: bytes 8..16. Mnemosyne lets you safely write up to
    // usable_size bytes, so this is well-defined; but the realloc copy
    // must not pull this into the new allocation.
    for i in 8..16usize {
        unsafe { ptr.add(i).write(0xBB) };
    }

    // Cross-class grow.
    let new_ptr = unsafe { allocator.realloc(ptr, small_layout, 64) };
    assert!(!new_ptr.is_null());

    for i in 0..8usize {
        assert_eq!(
            unsafe { new_ptr.add(i).read() },
            0xAA,
            "realloc must preserve the {i}-th user byte"
        );
    }
    for i in 8..16usize {
        assert_eq!(
            unsafe { new_ptr.add(i).read() },
            0,
            "secure realloc copied slack byte {i} past layout.size"
        );
    }

    let new_layout = Layout::from_size_align(64, 8).expect("valid layout");
    unsafe { allocator.dealloc(new_ptr, new_layout) };
}

#[test]
fn test_realloc_shrink_replacement_copies_only_new_size() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let allocator = MnemosyneAllocator::<SecurePolicy>::new();
    let old_layout = Layout::from_size_align(16 * 1024, 8).expect("valid layout");
    let new_size = 4 * 1024;
    let ptr = unsafe { allocator.alloc(old_layout) };
    assert!(!ptr.is_null(), "secure shrink setup allocation failed");

    for i in 0..new_size {
        unsafe { ptr.add(i).write((i as u8).wrapping_mul(17)) };
    }

    let new_ptr = unsafe { allocator.realloc(ptr, old_layout, new_size) };
    assert!(
        !new_ptr.is_null(),
        "secure shrink replacement allocation failed"
    );
    for i in 0..new_size {
        assert_eq!(
            unsafe { new_ptr.add(i).read() },
            (i as u8).wrapping_mul(17),
            "secure shrink failed to preserve byte {i}"
        );
    }

    let new_layout = Layout::from_size_align(new_size, 8).expect("valid layout");
    unsafe { allocator.dealloc(new_ptr, new_layout) };
}

#[test]
fn test_realloc_null_ptr_acts_as_alloc() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let layout = Layout::from_size_align(0, 8).expect("valid layout");
    let ptr = unsafe { ALLOCATOR.realloc(core::ptr::null_mut(), layout, 128) };
    assert!(!ptr.is_null(), "realloc(null, 128) must allocate");
    let new_layout = Layout::from_size_align(128, 8).expect("valid layout");
    unsafe { ALLOCATOR.dealloc(ptr, new_layout) };
}

#[test]
fn test_realloc_to_zero_size_frees() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let layout = Layout::from_size_align(32, 8).expect("valid layout");
    let ptr = unsafe { ALLOCATOR.alloc(layout) };
    assert!(!ptr.is_null());

    let null = unsafe { ALLOCATOR.realloc(ptr, layout, 0) };
    assert!(null.is_null(), "realloc(_, 0) must return null after free");
}

#[test]
fn test_large_alignment() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let alignments = [32 * 1024, 64 * 1024, 128 * 1024, 2 * 1024 * 1024];
    for align in alignments {
        let layout = Layout::from_size_align(4096, align)
            .expect("large-alignment test table contains valid Layout alignments");
        let ptr = unsafe { ALLOCATOR.alloc(layout) };
        assert!(!ptr.is_null(), "Allocation failed for alignment {}", align);
        assert_eq!(
            ptr as usize % align,
            0,
            "Pointer {:?} is not aligned to {}",
            ptr,
            align
        );
        // Verify writing and reading to make sure alignment bounds check out.
        unsafe {
            ptr.write(0xAA);
            assert_eq!(ptr.read(), 0xAA);
            ptr.add(4095).write(0x55);
            assert_eq!(ptr.add(4095).read(), 0x55);
        }
        unsafe { ALLOCATOR.dealloc(ptr, layout) };
    }
}

#[test]
fn test_secure_policy() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let allocator = MnemosyneAllocator::<SecurePolicy>::new();
    let layout = Layout::from_size_align(128, 8).expect("128-byte 8-aligned Layout is valid");

    // 1. Test zero-initialization
    let ptr = unsafe { allocator.alloc(layout) };
    assert!(!ptr.is_null(), "secure-policy allocation failed");

    // Verify that the memory is indeed zero-initialized
    let slice = unsafe { core::slice::from_raw_parts(ptr, 128) };
    for &byte in slice {
        assert_eq!(byte, 0, "Byte was not zero-initialized");
    }

    // 2. Test memory poisoning on deallocation.
    // We write some sentinel values before freeing to ensure it's overwritten by poison bytes.
    unsafe {
        core::ptr::write_bytes(ptr, 0x41, 128);
    }

    unsafe { allocator.dealloc(ptr, layout) };

    // Safety: Under standard execution, accessing freed memory is undefined behavior.
    // However, in this controlled integration test, we verify that the poisoning logic
    // has overwritten the memory. The segment cache retains pages so the memory
    // remains mapped and readable for testing.
    let skip_bytes =
        core::mem::size_of::<Option<core::ptr::NonNull<mnemosyne_core::types::Block>>>();
    for i in skip_bytes..128 {
        let val = unsafe { ptr.add(i).read() };
        assert_eq!(
            val, 0xDE,
            "Byte at index {} was not poisoned (got 0x{:02X}, expected 0xDE)",
            i, val
        );
    }
}

#[test]
fn test_cuda_unified_backend() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let allocator = MnemosyneAllocator::<StandardPolicy, CudaUnifiedBackend>::new();
    let layout = Layout::from_size_align(128, 8).expect("128-byte 8-aligned Layout is valid");
    let ptr = unsafe { allocator.alloc(layout) };
    assert!(!ptr.is_null(), "CUDA unified backend allocation failed");

    unsafe {
        ptr.write(42);
        assert_eq!(ptr.read(), 42);
        allocator.dealloc(ptr, layout);
    }

    // Verify statistics generic query works for CUDA backend
    let stats = memory_stats_generic::<CudaUnifiedBackend>();
    assert_eq!(stats.current_thread_live_allocations, 0);

    // Verify is_cuda_available is callable
    let _ = is_cuda_available();
}

#[test]
fn test_leak_detector_integration() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    mnemosyne_prof::reset_profiler_for_testing();

    thread::spawn(|| {
        enable_leak_detector();
        assert!(is_leak_detector_enabled());

        let layout = Layout::from_size_align(64, 8).expect("valid layout");
        let ptr = unsafe { ALLOCATOR.alloc(layout) };
        assert!(!ptr.is_null());

        disable_leak_detector();
        assert!(!is_leak_detector_enabled());

        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("mnemosyne_integration_leaks.txt");
        let path_str = path.to_str().expect("valid temp path");

        let dump_res = dump_leaks(path_str);
        assert!(dump_res.is_ok(), "dump_leaks failed: {:?}", dump_res.err());
        let count = dump_res.unwrap();
        assert!(
            count >= 1,
            "Expected at least 1 leak captured (got {})",
            count
        );

        // Verify the file was created and contains the backtrace info.
        let content = std::fs::read_to_string(&path).expect("failed to read leak report");
        assert!(
            content.contains("test_leak_detector_integration"),
            "Stack trace missing integration test function symbol: {}",
            content
        );

        let _ = std::fs::remove_file(&path);
        unsafe { ALLOCATOR.dealloc(ptr, layout) };
        mnemosyne_prof::reset_profiler_for_testing();
    })
    .join()
    .expect("leak detector integration thread panicked");
    mnemosyne_prof::reset_profiler_for_testing();
}
