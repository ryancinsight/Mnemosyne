use super::*;

#[test]
fn test_heap_allocation_and_free() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let layout = test_layout(32, 8);
        let block = heap.alloc(&token, layout).expect("heap allocation failed");
        let ptr = block.as_ptr();

        unsafe {
            ptr.write(42);
            assert_eq!(ptr.read(), 42);
        }

        heap.free(&mut token, block);
    });
}

#[test]
fn test_heap_realloc() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let layout = test_layout(16, 8);
        let block = heap.alloc(&token, layout).expect("heap allocation failed");
        let ptr = block.as_ptr();

        unsafe {
            ptr.write(99);
        }

        let new_block = heap
            .realloc(&mut token, block, layout, 32)
            .expect("heap realloc failed");
        let new_ptr = new_block.as_ptr();
        unsafe {
            assert_eq!(new_ptr.read(), 99);
        }

        heap.free_uninit(&mut token, new_block);
    });
}

#[test]
fn test_branded_heap_allocation_and_free() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let layout = test_layout(32, 8);
        let block = heap
            .alloc(&token, layout)
            .expect("branded allocation failed");
        let ptr = block.as_ptr();
        assert!(!ptr.is_null());
        unsafe {
            ptr.write(42);
            assert_eq!(ptr.read(), 42);
        }
        heap.free(&mut token, block);
    });
}

#[test]
fn test_branded_heap_realloc() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let layout = test_layout(16, 8);
        let block = heap
            .alloc(&token, layout)
            .expect("branded allocation failed");
        let ptr = block.as_ptr();
        unsafe {
            ptr.write(99);
        }
        let new_block = heap
            .realloc(&mut token, block, layout, 32)
            .expect("branded realloc failed");
        let new_ptr = new_block.as_ptr();
        unsafe {
            assert_eq!(new_ptr.read(), 99);
        }
        heap.free(&mut token, new_block);
    });
}

#[test]
fn test_branded_heap_realloc_zst_to_nonzero_skips_source_free() {
    struct Marker;

    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let before = heap.stats();
        let block = heap.alloc_init(&token, Marker).expect("ZST alloc failed");
        let after_zst_alloc = heap.stats();

        assert_eq!(
            after_zst_alloc.current_thread_live_allocations, before.current_thread_live_allocations,
            "ZST source construction must not create a live allocator block"
        );

        let new_block = heap
            .realloc(&mut token, block, Layout::new::<Marker>(), 16)
            .expect("ZST-to-nonzero realloc failed");
        let after_alloc = heap.stats();

        assert!(
            !new_block.as_ptr().is_null(),
            "realloc returned a null block"
        );
        assert!(
            after_alloc.current_thread_live_allocations
                > after_zst_alloc.current_thread_live_allocations,
            "nonzero destination must create a live allocator block"
        );

        heap.free_uninit(&mut token, new_block);
        let after_free = heap.stats();
        assert!(
            after_free.current_thread_live_allocations
                < after_alloc.current_thread_live_allocations,
            "free_uninit must release the nonzero destination block"
        );
    });
}

#[test]
fn test_branded_heap_realloc_zst_to_zero_drops_without_allocating() {
    ZST_DROP_COUNT.with(|c| c.set(0));
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let before = heap.stats();
        let block = heap
            .alloc_init(&token, ZstDrop)
            .expect("ZST alloc_init failed");

        let result = heap.realloc(&mut token, block, Layout::new::<ZstDrop>(), 0);
        let after = heap.stats();

        assert!(
            result.is_none(),
            "ZST-to-zero realloc must consume the block without a replacement"
        );
        assert_eq!(
            after.current_thread_live_allocations, before.current_thread_live_allocations,
            "ZST-to-zero realloc must not allocate or free a real block"
        );
        assert_eq!(
            ZST_DROP_COUNT.with(|c| c.get()),
            1,
            "ZST-to-zero realloc must drop the owned value exactly once"
        );
    });
}

#[test]
fn test_branded_heap_generic_and_cast() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let layout = test_layout(32, 8);
        let block: BrandedBlock<'_, u8> = heap
            .alloc(&token, layout)
            .expect("branded allocation failed");

        // Cast to i32 block
        let casted: BrandedBlock<'_, i32> = block.cast::<i32>();
        let ptr = casted.as_ptr();
        assert!(!ptr.is_null());
        unsafe {
            ptr.write(123456);
            assert_eq!(ptr.read(), 123456);
        }

        // Free generic block
        heap.free(&mut token, casted);
    });
}

#[test]
fn test_branded_heap_free_drops_value() {
    let counter = AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let block = heap
            .alloc_init(&token, DropTracker(&counter))
            .expect("alloc_init failed");
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        heap.free(&mut token, block);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn test_branded_heap_alloc_init_zst_drops_without_allocating() {
    ZST_DROP_COUNT.with(|c| c.set(0));
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let before = heap.stats().current_thread_owned_segments;
        let block = heap
            .alloc_init(&token, ZstDrop)
            .expect("ZST alloc_init failed");
        assert_eq!(
            heap.stats().current_thread_owned_segments,
            before,
            "ZST alloc_init must not allocate a segment"
        );
        heap.free(&mut token, block);
        assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 1);
    });
}
