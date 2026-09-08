//! Reported usable size against the block, class, and mapping bounds.

use super::*;

#[test]
fn usable_size_returns_block_size_for_small_allocations() {
    // Mnemosyne rounds small allocation requests up to the next
    // size class, so the usable size should match `class_to_size`
    // for every (request, alignment) pair the small-alloc test
    // sweep exercises, regardless of the *requested* size.
    for &(req_size, req_align) in &[(8usize, 8usize), (16, 8), (32, 16), (64, 8), (1024, 8)] {
        let ptr =
            unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(req_size, req_align) };
        assert!(
            !ptr.is_null(),
            "alloc({req_size}, {req_align}) returned null"
        );

        let reported = unsafe { usable_size(ptr) };
        assert!(
            reported >= req_size,
            "usable_size({req_size}, {req_align}) = {reported} is below the request"
        );
        assert!(
            reported >= req_align,
            "usable_size({req_size}, {req_align}) = {reported} is below the adjusted minimum (alignment)"
        );
        // The reported size is whatever size class the page is
        // sliced into; verify it matches a real class.
        let ptr_val = ptr as usize;
        let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
        let page = unsafe { &(*segment).pages[page_index] };
        assert_eq!(
            reported, page.block_size,
            "usable_size disagrees with the page's recorded block_size"
        );

        unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
    }
}

#[test]
fn usable_size_never_under_reports_across_every_size_class() {
    // The lower-bound counterpart to
    // `usable_size_does_not_over_report_past_mapping_end_for_huge_allocations`.
    // An under-report is the more dangerous direction for small
    // allocations: a `Vec` that trusts `usable_size` to compute spare
    // capacity would write past the reported window and corrupt an
    // adjacent block. Exhaustively prove `usable_size(ptr) >=
    // requested_size` for at least one representative request in every
    // small size class, plus the inter-class boundary bytes that the
    // size-class mapper rounds.
    use mnemosyne_core::NUM_SIZE_CLASSES;
    use mnemosyne_core::size_class::class_to_size;

    for class in 0..NUM_SIZE_CLASSES {
        let class_max = class_to_size(class);
        // Exercise the smallest request that lands in this class
        // (one byte past the previous class's max) and the class max
        // itself. Both must report at least the requested size.
        let prev_max = if class == 0 {
            0
        } else {
            class_to_size(class - 1)
        };
        for &req in &[prev_max + 1, class_max] {
            let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(req, 8) };
            assert!(
                !ptr.is_null(),
                "alloc({req}) returned null for class {class}"
            );

            let reported = unsafe { usable_size(ptr) };
            assert!(
                reported >= req,
                "usable_size under-reported for class {class}: requested {req}, got {reported}"
            );
            // The reported value is the class block size, which must
            // be exactly `class_max` for any request in this class.
            assert_eq!(
                reported, class_max,
                "usable_size for request {req} (class {class}) should equal class max {class_max}"
            );

            unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
        }
    }
}

#[test]
fn usable_size_returns_payload_remainder_for_huge_allocations() {
    let _guard = crate::local_alloc::TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    // Direct large allocation through the arena. The returned
    // pointer carries enough payload to cover the requested size,
    // and `usable_size` reports at least that much (it may report
    // more because the arena reserves alignment slack).
    let request = 4 * 1024 * 1024;
    for &align in &[8usize, 64 * 1024, 1024 * 1024, SEGMENT_SIZE] {
        // SAFETY: power-of-two alignment, non-zero size.
        let ptr = unsafe {
            mnemosyne_arena::allocate_large_or_huge::<MemoryBackendWrapper>(request, align, true)
        };
        assert!(!ptr.is_null(), "huge allocation failed for align {align}");

        let reported = unsafe { usable_size(ptr) };
        assert!(
            reported >= request,
            "usable_size = {reported} is below the requested huge size {request} for align {align}"
        );

        let recovered = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        let _released = unsafe {
            mnemosyne_arena::deallocate_large_or_huge::<MemoryBackendWrapper>(ptr, recovered)
        };
    }

    // `deallocate_large_or_huge` returns the mapping to the global huge pool,
    // which retains it for reuse. That retention is by design but reads as a
    // leak at process exit, so drain it here — the same convention the arena
    // and cross-thread tests already follow.
    // SAFETY: the test lock is held, so no other test mutates the pool.
    unsafe { mnemosyne_arena::purge_segment_pool::<MemoryBackendWrapper>() };
}

#[test]
fn usable_size_does_not_over_report_past_mapping_end_for_huge_allocations() {
    let _guard = crate::local_alloc::TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    // Strict assertion that catches the SEGMENT_ALIGN-1 byte over-report
    // that resulted from using segment_ptr (aligned_addr) as the
    // mapping base instead of segment.raw_alloc_ptr. We compute the
    // distance from ptr to the end of the *actual* OS mapping
    // (raw_alloc_ptr + huge_size) and assert usable_size never exceeds it.
    let request = 4 * 1024 * 1024;
    for &align in &[8usize, 64 * 1024, 1024 * 1024, SEGMENT_SIZE] {
        // SAFETY: power-of-two alignment, non-zero size.
        let ptr = unsafe {
            mnemosyne_arena::allocate_large_or_huge::<MemoryBackendWrapper>(request, align, true)
        };
        assert!(!ptr.is_null(), "huge allocation failed for align {align}");

        let recovered = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        let huge_size = unsafe { (*recovered).pages[0].block_size };
        let raw_ptr = unsafe { (*recovered).raw_alloc_ptr } as usize;
        let mapping_end = raw_ptr + huge_size;
        let actual_remaining = mapping_end - ptr as usize;

        let reported = unsafe { usable_size(ptr) };
        assert!(
            reported <= actual_remaining,
            "usable_size {} exceeds remaining mapping {} (raw_ptr={:#x}, ptr={:?}, huge_size={}) for align {align}",
            reported,
            actual_remaining,
            raw_ptr,
            ptr,
            huge_size,
        );
        assert!(
            reported >= request,
            "usable_size {} is below requested {} for align {align}",
            reported,
            request,
        );

        let _released = unsafe {
            mnemosyne_arena::deallocate_large_or_huge::<MemoryBackendWrapper>(ptr, recovered)
        };
    }

    // `deallocate_large_or_huge` returns the mapping to the global huge pool,
    // which retains it for reuse. That retention is by design but reads as a
    // leak at process exit, so drain it here — the same convention the arena
    // and cross-thread tests already follow.
    // SAFETY: the test lock is held, so no other test mutates the pool.
    unsafe { mnemosyne_arena::purge_segment_pool::<MemoryBackendWrapper>() };
}

#[test]
fn usable_size_ignores_payload_bytes_at_a_segment_aligned_huge_pointer() {
    let _guard = crate::local_alloc::TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    // A huge allocation whose alignment request lands the payload on a segment
    // boundary makes `locate_segment` mask down to an address that holds user
    // data, not a segment header. The classifier used to read `block_size` from
    // there before checking the index, so the caller's own bytes decided
    // whether the allocation looked small. Fill the payload with a pattern that
    // reads as a non-zero `block_size` and confirm the size still comes from
    // the metadata slot.
    let request = 4 * 1024 * 1024;
    let ptr = unsafe {
        mnemosyne_arena::allocate_large_or_huge::<MemoryBackendWrapper>(request, SEGMENT_SIZE, true)
    };
    assert!(!ptr.is_null(), "segment-aligned huge allocation failed");
    assert_eq!(
        ptr as usize % SEGMENT_SIZE,
        0,
        "this test is only meaningful when the payload is segment-aligned"
    );

    // SAFETY: `ptr` owns `request` writable bytes.
    unsafe { core::ptr::write_bytes(ptr, 0xAB, request) };

    let recovered = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
    let huge_size = unsafe { (*recovered).pages[0].block_size };
    let mapping_end = unsafe { (*recovered).raw_alloc_ptr } as usize + huge_size;
    let actual_remaining = mapping_end - ptr as usize;

    let reported = unsafe { usable_size(ptr) };
    // The over-report is the direction that matters: misreading the payload as
    // page metadata yields whatever the caller happened to store there, which
    // is unbounded. A caller sizing spare capacity from it writes off the end
    // of the mapping.
    assert!(
        reported <= actual_remaining,
        "usable_size {reported} exceeds the remaining mapping \n         {actual_remaining}; payload bytes were read as page metadata"
    );
    assert!(
        reported >= request,
        "usable_size {reported} is below the requested {request}"
    );

    let _released = unsafe {
        mnemosyne_arena::deallocate_large_or_huge::<MemoryBackendWrapper>(ptr, recovered)
    };

    // `deallocate_large_or_huge` returns the mapping to the global huge pool,
    // which retains it for reuse. That retention is by design but reads as a
    // leak at process exit, so drain it here — the same convention the arena
    // and cross-thread tests already follow.
    // SAFETY: the test lock is held, so no other test mutates the pool.
    unsafe { mnemosyne_arena::purge_segment_pool::<MemoryBackendWrapper>() };
}

#[test]
fn usable_size_returns_zero_for_null_pointer() {
    let reported = unsafe { usable_size(core::ptr::null_mut()) };
    assert_eq!(reported, 0);
}
