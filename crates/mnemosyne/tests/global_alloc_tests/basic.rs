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
fn alloc_free_alloc_refreshes_page_metadata_provenance() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let layout = Layout::from_size_align(64, 32).expect("64-byte, 32-aligned layout is valid");

    unsafe {
        let retained = ALLOCATOR.alloc(layout);
        let released = ALLOCATOR.alloc(layout);
        assert!(!retained.is_null() && !released.is_null());
        retained.write_bytes(0xA5, layout.size());
        released.write_bytes(0x5A, layout.size());

        ALLOCATOR.dealloc(released, layout);
        let reused = ALLOCATOR.alloc(layout);
        assert!(!reused.is_null());
        let retained_bytes_unchanged = core::slice::from_raw_parts(retained, layout.size())
            .iter()
            .all(|&byte| byte == 0xA5);
        let reused_released_block = reused == released;

        ALLOCATOR.dealloc(reused, layout);
        ALLOCATOR.dealloc(retained, layout);

        assert!(
            retained_bytes_unchanged,
            "metadata mutation changed a distinct live allocation"
        );
        assert!(
            reused_released_block,
            "same-class local free must be the next block reused"
        );
    }
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
fn test_cross_thread_free_is_reclaimed_by_owner_page_drain() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");

    unsafe {
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }

    let layout = Layout::from_size_align(32, 8).expect("32-byte, 8-aligned layout is valid");
    let ptr = unsafe { ALLOCATOR.alloc(layout) };
    assert!(
        !ptr.is_null(),
        "owner allocation for cross-thread free test failed"
    );

    let ptr_addr = ptr as usize;
    let handle = thread::spawn(move || {
        // SAFETY: this pointer was allocated by the owning thread, and the
        // non-owner thread is only enqueueing it for owner-side reclaim.
        unsafe {
            ALLOCATOR.dealloc(ptr_addr as *mut u8, layout);
        }
    });
    handle.join().expect("cross-thread free worker panicked");

    // The owning thread must reclaim the queued block only when it re-drains the
    // page; the original pointer should reappear in the normal allocation stream
    // after enough probe allocations to force the page drain.
    let segment_addr = ptr as usize & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut mnemosyne_local::internal::Segment;
    let page_index = (ptr as usize >> mnemosyne_core::constants::PAGE_SHIFT)
        & (mnemosyne_core::constants::PAGES_PER_SEGMENT - 1);
    let max_blocks = unsafe { (*segment).pages[page_index].max_blocks() };

    let mut probe_allocations = std::vec::Vec::with_capacity(max_blocks);
    let mut reclaimed = false;
    for probe_index in 0..max_blocks {
        let probe = unsafe { ALLOCATOR.alloc(layout) };
        assert!(
            !probe.is_null(),
            "owner page-drain probe allocation failed before reclaiming remote free"
        );
        probe_allocations.push(probe);

        // A remote free can only be reclaimed by the owning thread when the page
        // is drained; the non-owner thread must not recycle the block by itself.
        if probe == ptr {
            reclaimed = true;
            break;
        }

        assert_ne!(
            probe, ptr,
            "remote free must stay pending until the owning thread drains the page; probe_index={} max_blocks={}",
            probe_index, max_blocks
        );
    }

    assert!(
        reclaimed,
        "cross-thread freed block was not reclaimed by the owner page drain after {} probes",
        max_blocks
    );

    unsafe {
        for probe in probe_allocations {
            ALLOCATOR.dealloc(probe, layout);
        }
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

#[test]
fn test_small_aligned_allocations_are_aligned() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    // Alignments above MIN_BLOCK_SIZE (16) on the small path must return
    // correctly aligned, usable memory. Sizes include ones whose natural size
    // class has a non-power-of-two stride (40->48, 96, 100->112, 400->416),
    // which must still honour the alignment (via a class bump or the huge
    // fallback) — a misaligned return here would be UB for SIMD consumers.
    for &align in &[16usize, 32, 64, 128, 256] {
        for &size in &[1usize, 8, 40, 48, 50, 96, 100, 200, 256, 400, 1000, 4096] {
            let layout = Layout::from_size_align(size, align).expect("valid layout");
            unsafe {
                let p = ALLOCATOR.alloc(layout);
                assert!(!p.is_null(), "alloc failed size={size} align={align}");
                assert_eq!(
                    (p as usize) & (align - 1),
                    0,
                    "pointer not {align}-aligned for size={size}"
                );
                // Write and read back the full requested range to prove the
                // block is usable for `size` bytes at the requested alignment.
                core::ptr::write_bytes(p, 0xAB, size);
                assert_eq!(*p, 0xAB);
                assert_eq!(*p.add(size - 1), 0xAB);
                ALLOCATOR.dealloc(p, layout);
            }
        }
    }
}
