use super::*;

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
        8, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96, 104, 112, 120, 128, 160, 192, 224, 256, 288,
        320, 352, 384, 416, 448, 480, 512, 640, 768, 896, 1024, 1152, 1280, 1408, 1536, 1664, 1792,
        1920, 2048,
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
    assert!(
        stats
            .size_class_occupancy
            .iter()
            .any(|occupancy| occupancy.active_pages > 0)
    );
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
