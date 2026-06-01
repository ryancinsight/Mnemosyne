use core::alloc::Layout;
use mnemosyne_backend::MemoryBackendWrapper;
use mnemosyne_core::StandardPolicy;
use mnemosyne_heap::scope;

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn test_layout(size: usize, align: usize) -> Layout {
    Layout::from_size_align(size, align)
        .expect("heap integration test layout must use a nonzero power-of-two alignment")
}

#[test]
fn test_multi_heap_basic() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let layout = test_layout(32, 8);
        let block = heap.alloc(&token, layout).expect("heap allocation failed");
        let ptr = block.as_ptr();
        unsafe { ptr.write(123) };
        assert_eq!(unsafe { ptr.read() }, 123);

        let block = heap
            .realloc(&mut token, block, layout, 64)
            .expect("heap realloc failed");
        assert_eq!(unsafe { block.as_ptr().read() }, 123);

        heap.free_uninit(&mut token, block);
    });
}

#[test]
fn test_runtime_options_override_default_retention() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use mnemosyne_arena::HasSegmentPool;

    // Reset options to default
    mnemosyne_local::reset_options_for_testing();

    // 1. Force the option to 0 via env var
    std::env::set_var("MNEMOSYNE_MAX_RETAINED_SEGMENTS", "0");

    let pool = <MemoryBackendWrapper as HasSegmentPool>::global_segment_pool();
    unsafe {
        mnemosyne_arena::purge_segment_pool::<MemoryBackendWrapper>();
    }
    let initial_retained = pool.retained_count();

    // Do an allocation via Heap to trigger options parsing and then drop it.
    {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let layout = test_layout(32, 8);
            let block = heap.alloc(&token, layout).expect("heap allocation failed");
            heap.free_uninit(&mut token, block);
        });
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
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap1, mut token1| {
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap2, mut token2| {
            let layout = test_layout(32, 8);
            let block1 = heap1
                .alloc(&token1, layout)
                .expect("heap1 allocation failed");
            let block2 = heap2
                .alloc(&token2, layout)
                .expect("heap2 allocation failed");
            let ptr1 = block1.as_ptr();
            let ptr2 = block2.as_ptr();

            assert_ne!(ptr1, ptr2);

            unsafe {
                ptr1.write(111);
                ptr2.write(222);

                assert_eq!(ptr1.read(), 111);
                assert_eq!(ptr2.read(), 222);
            }

            heap1.free_uninit(&mut token1, block1);
            heap2.free_uninit(&mut token2, block2);
        });
    });
}

#[test]
fn multi_heap_release_does_not_touch_other_heaps() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use mnemosyne_arena::HasSegmentPool;

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

    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap2, mut token2| {
        let layout = test_layout(32, 8);
        let block2 = heap2
            .alloc(&token2, layout)
            .expect("heap2 allocation failed");
        unsafe {
            block2.as_ptr().write(77);
        }

        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap1, mut token1| {
            let block1 = heap1
                .alloc(&token1, layout)
                .expect("heap1 allocation failed");
            unsafe {
                block1.as_ptr().write(55);
            }
            heap1.free_uninit(&mut token1, block1);
        });

        // Dropping heap1 with 0 live allocations causes its segment to be returned to the global pool.
        // Retained segments should increase.
        let retained_after_heap1 = pool.retained_count();
        assert!(
            retained_after_heap1 > initial_retained,
            "Segment from heap1 should have been returned to global pool"
        );

        // Verify heap2 is completely untouched and block2 is still valid.
        assert_eq!(unsafe { block2.as_ptr().read() }, 77);

        heap2.free_uninit(&mut token2, block2);
    });

    mnemosyne_local::reset_options_for_testing();
}

#[test]
fn test_programmatic_options_configure() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use mnemosyne_arena::HasSegmentPool;

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
        scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
            let layout = test_layout(32, 8);
            let block = heap.alloc(&token, layout).expect("heap allocation failed");
            heap.free_uninit(&mut token, block);
        });
    }

    let final_retained = pool.retained_count();
    assert_eq!(final_retained, initial_retained);

    // Reset options
    mnemosyne_local::reset_options_for_testing();
}
