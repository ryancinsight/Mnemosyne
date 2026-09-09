//! Allocation and free routing through the thread-local caches.

use super::*;

#[test]
fn small_alloc_returns_block_aligned_ptr_outside_metadata_page() {
    // The small-free classifier in `thread_free` relies on three
    // invariants: `page_index >= 1`, `page_index < PAGES_PER_SEGMENT`,
    // and `(ptr - page_start) % page.block_size == 0`. Verify each one
    // against the live allocation grid that customers actually observe.
    for &(req_size, req_align) in &[(8usize, 8usize), (16, 8), (32, 16), (64, 8), (1024, 8)] {
        let ptr =
            unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(req_size, req_align) };
        assert!(
            !ptr.is_null(),
            "alloc({req_size}, {req_align}) returned null"
        );

        let ptr_val = ptr as usize;
        let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

        assert!(
            page_index >= 1,
            "alloc({req_size}, {req_align}) ptr {ptr:?} landed in metadata Page 0"
        );
        assert!(
            page_index < PAGES_PER_SEGMENT,
            "alloc({req_size}, {req_align}) page_index {page_index} >= PAGES_PER_SEGMENT"
        );
        let page = unsafe { &(*segment).pages[page_index] };
        assert!(
            page.block_size > 0,
            "alloc({req_size}, {req_align}) targeted an uninitialized page"
        );
        let block_stride = page.block_size as usize;
        let offset = ptr_val & (PAGE_SIZE - 1);
        assert_eq!(
            offset % block_stride,
            0,
            "alloc({req_size}, {req_align}) ptr is not aligned to block stride {block_stride} of its size class",
        );

        unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
    }
}

#[test]
fn reentrant_current_segment_local_free_uses_metadata_fast_path() {
    let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(32, 8) };
    assert!(
        !ptr.is_null(),
        "reentrant local-free setup allocation failed"
    );

    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    // Raw, re-derived per assertion: the free below writes this page through
    // the allocator's own tag, so a `&mut Page` held across it would be
    // invalidated and every later read through it stale.
    let page = unsafe { &raw mut (*segment).pages[page_index] };

    assert_eq!(unsafe { (*page).alloc_count }, 1);
    assert!(
        unsafe { (*page).thread_free.is_empty() },
        "thread_free list should start empty before reentrant free"
    );

    MemoryBackendWrapper::with_allocator(|_| {
        unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
    });

    assert_eq!(unsafe { (*page).alloc_count }, 0);
    assert!(
        unsafe { (*page).thread_free.is_empty() },
        "current-segment local free should not enqueue into page-local thread_free"
    );
    assert_eq!(
        unsafe { (*page).free }.map(NonNull::as_ptr),
        Some(ptr as *mut Block)
    );
}

#[test]
fn current_segment_free_keeps_occupancy_mask_conservative() {
    let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(32, 8) };
    assert!(!ptr.is_null(), "current-segment mask allocation failed");

    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    let mask = 1u32 << page_index;

    assert!(
        unsafe { Segment::is_current(segment) },
        "test allocation must come from the current slicing segment"
    );
    assert_ne!(
        unsafe { (*segment).page_occupied_mask } & mask,
        0,
        "allocation must mark the owning page occupied"
    );

    unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };

    assert_eq!(unsafe { (*segment).pages[page_index].alloc_count }, 0);
    assert_ne!(
        unsafe { (*segment).page_occupied_mask } & mask,
        0,
        "current-segment free keeps a conservative mask bit for hot reuse"
    );
}

#[test]
fn thread_alloc_cold_charges_one_defrag_operation_per_page_refill() {
    let _guard = crate::local_alloc::TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let worker = std::thread::spawn(|| {
        let before = MemoryBackendWrapper::with_allocator(|alloc| {
            assert_eq!(
                alloc.page_refills, 0,
                "fresh worker allocator should start with no page refills"
            );
            alloc.defrag_counter
        })
        .expect("fresh worker allocator slot must initialize");

        let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(8192, 8) };
        assert!(!ptr.is_null(), "8192-byte allocation failed");

        let after = MemoryBackendWrapper::with_allocator(|alloc| {
            (alloc.defrag_counter, alloc.page_refills)
        })
        .expect("worker allocator slot must remain accessible");

        unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };

        (before, after.0, after.1)
    });

    let (before, after, refills) = worker
        .join()
        .expect("defrag accounting worker thread panicked");
    assert_eq!(refills, 1, "single cold allocation should refill one page");
    assert_eq!(
        after,
        before + 1,
        "single page refill should charge exactly one defrag operation"
    );
}

#[test]
fn hardened_policy_round_trip_alloc_free() {
    use mnemosyne_core::policy::HardenedPolicy;

    let _guard = crate::local_alloc::TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let ptr = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(32, 8) };
    assert!(!ptr.is_null(), "HardenedPolicy small allocation failed");

    // Verify that the memory is zero-initialized (since HardenedPolicy inherits from SecurePolicy, which zero-initializes)
    let slice = unsafe { core::slice::from_raw_parts(ptr, 32) };
    for &byte in slice {
        assert_eq!(
            byte, 0,
            "HardenedPolicy allocation was not zero-initialized"
        );
    }

    // Verify that we can write to it
    unsafe {
        core::ptr::write_bytes(ptr, 0x42, 32);
    }

    // Free the pointer
    unsafe {
        thread_free::<HardenedPolicy, MemoryBackendWrapper>(ptr);
    }
}

#[test]
fn test_dealloc_path() {
    let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(1024, 8) };
    assert!(!ptr.is_null());
    unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
}
