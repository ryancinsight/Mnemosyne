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
            .expect("heap realloc returned an error")
            .expect("heap realloc did not return a replacement block");
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
            .expect("branded realloc returned an error")
            .expect("branded realloc did not return a replacement block");
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
            .expect("ZST-to-nonzero realloc returned an error")
            .expect("ZST-to-nonzero realloc did not return a replacement block");
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
            result
                .expect("ZST-to-zero realloc returned an unexpected error")
                .is_none(),
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
fn test_branded_heap_realloc_invalid_layout_returns_source_block() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let layout = test_layout(16, 8);
        let block = heap
            .alloc(&token, layout)
            .expect("source allocation for invalid realloc failed");
        unsafe { block.as_ptr().write(0x5A) };

        let error = match heap.realloc(&mut token, block, layout, usize::MAX) {
            Ok(_) => panic!("invalid realloc layout unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(
            error.reason(),
            ReallocFailure::InvalidLayout {
                new_size: usize::MAX,
                alignment: 8,
            },
            "invalid realloc must report the rejected layout"
        );

        let source = error.into_block();
        assert_eq!(
            unsafe { source.as_ptr().read() },
            0x5A,
            "failed realloc must preserve the source allocation"
        );
        heap.free_uninit(&mut token, source);
    });
}

#[test]
fn test_branded_heap_realloc_rejects_oversized_source_layout() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let source_layout = test_layout(1, 1);
        let block = heap
            .alloc(&token, source_layout)
            .expect("source allocation for layout validation failed");
        unsafe { block.as_ptr().write(0xA5) };
        let usable_size = unsafe { mnemosyne_local::usable_size(block.as_ptr()) };
        let requested_size = usable_size
            .checked_add(1)
            .expect("allocator usable size must leave room for a test mismatch");
        let oversized_layout = test_layout(requested_size, source_layout.align());

        let error = match heap.realloc(&mut token, block, oversized_layout, requested_size) {
            Ok(_) => panic!("oversized source layout unexpectedly succeeded"),
            Err(error) => error,
        };
        assert_eq!(
            error.reason(),
            ReallocFailure::InvalidSourceLayout {
                requested_size,
                alignment: source_layout.align(),
                usable_size,
            },
            "realloc must report the source-layout capacity mismatch"
        );

        let source = error.into_block();
        assert_eq!(
            unsafe { source.as_ptr().read() },
            0xA5,
            "source bytes must survive source-layout rejection"
        );
        heap.free_uninit(&mut token, source);
    });
}

#[test]
fn test_branded_heap_generic_and_cast() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let layout = test_layout(32, 8);
        let block: BrandedBlock<'_, u8> = heap
            .alloc(&token, layout)
            .expect("branded allocation failed");

        // SAFETY: `block` is a freshly allocated 32-byte block with alignment
        // 8, so it is sized and aligned for `i32`; the `write` below
        // initializes a valid `i32` before `free` drops it as one (`i32` has
        // no drop glue, and the raw free path derives the deallocation from
        // the pointer's owning page, not from `T`).
        let casted: BrandedBlock<'_, i32> = unsafe { block.cast::<i32>() };
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
