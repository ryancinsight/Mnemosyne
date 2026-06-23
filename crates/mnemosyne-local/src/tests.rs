use super::*;
use core::ptr::NonNull;
use mnemosyne_backend::MemoryBackendWrapper;
use mnemosyne_core::constants::{
    MAX_ALLOC_SIZE, PAGES_PER_SEGMENT, PAGE_SHIFT, PAGE_SIZE, SEGMENT_SIZE,
};
use mnemosyne_core::policy::StandardPolicy;
use mnemosyne_core::types::{Block, Segment};

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
    use mnemosyne_core::size_class::class_to_size;
    use mnemosyne_core::NUM_SIZE_CLASSES;

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
    // Direct large allocation through the arena. The returned
    // pointer carries enough payload to cover the requested size,
    // and `usable_size` reports at least that much (it may report
    // more because the arena reserves alignment slack).
    let request = 4 * 1024 * 1024;
    for &align in &[8usize, 64 * 1024, 1024 * 1024, SEGMENT_SIZE] {
        // Safety: power-of-two alignment, non-zero size.
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
}

#[test]
fn usable_size_does_not_over_report_past_mapping_end_for_huge_allocations() {
    // Strict assertion that catches the SEGMENT_ALIGN-1 byte over-report
    // that resulted from using segment_ptr (aligned_addr) as the
    // mapping base instead of segment.raw_alloc_ptr. We compute the
    // distance from ptr to the end of the *actual* OS mapping
    // (raw_alloc_ptr + huge_size) and assert usable_size never exceeds it.
    let request = 4 * 1024 * 1024;
    for &align in &[8usize, 64 * 1024, 1024 * 1024, SEGMENT_SIZE] {
        // Safety: power-of-two alignment, non-zero size.
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
}

#[test]
fn usable_size_returns_zero_for_null_pointer() {
    let reported = unsafe { usable_size(core::ptr::null_mut()) };
    assert_eq!(reported, 0);
}

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
        let offset = ptr_val & (PAGE_SIZE - 1);
        assert_eq!(
            offset % page.block_size,
            0,
            "alloc({req_size}, {req_align}) ptr is not aligned to block stride {} of its size class",
            page.block_size,
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
    let page = unsafe { &mut (*segment).pages[page_index] };

    assert_eq!(page.alloc_count, 1);
    assert!(
        page.thread_free.is_empty(),
        "thread_free list should start empty before reentrant free"
    );

    MemoryBackendWrapper::with_allocator(|_| {
        unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
    });

    assert_eq!(page.alloc_count, 0);
    assert!(
        page.thread_free.is_empty(),
        "current-segment local free should not enqueue into page-local thread_free"
    );
    assert_eq!(page.free.map(NonNull::as_ptr), Some(ptr as *mut Block));
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
        unsafe { (*segment).is_current },
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
fn thread_alloc_rejects_invalid_alignment_requests() {
    for &align in &[0usize, 3, 6, 12, SEGMENT_SIZE * 2] {
        let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(64, align) };
        assert!(
            ptr.is_null(),
            "invalid alignment {align} should be rejected"
        );
    }
}

#[test]
fn thread_alloc_rejects_zero_size_requests() {
    for &align in &[1usize, 8, 16, PAGE_SIZE] {
        let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(0, align) };
        assert!(ptr.is_null(), "zero-size allocation should be rejected");
    }
}

#[test]
fn thread_alloc_rejects_size_above_layout_bound() {
    let ptr =
        unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(MAX_ALLOC_SIZE + 1, 8) };
    assert!(
        ptr.is_null(),
        "above-MAX_ALLOC_SIZE thread_alloc returned {ptr:?}"
    );
}

#[test]
fn thread_alloc_layout_uses_layout_validated_fast_entry() {
    let ptr = unsafe { thread_alloc_layout::<StandardPolicy, MemoryBackendWrapper>(64, 8) };
    assert!(
        !ptr.is_null(),
        "Layout-validated thread_alloc fast entry returned null"
    );
    unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };

    let oversized = unsafe {
        thread_alloc_layout::<StandardPolicy, MemoryBackendWrapper>(64, SEGMENT_SIZE * 2)
    };
    assert!(
        oversized.is_null(),
        "Layout-validated oversized alignment returned {oversized:?}"
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
    use mnemosyne_hardened::HardenedPolicy;

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
fn hardened_policy_detects_freelist_tamper() {
    use mnemosyne_hardened::HardenedPolicy;

    let _guard = crate::local_alloc::TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    // We want to verify that tamper detection works under HardenedPolicy.
    // Let's allocate two blocks on a fresh page of class 0 (16 bytes).
    // Since we want them on the same page, we can allocate them in sequence.
    let ptr1 = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(16, 8) };
    let ptr2 = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(16, 8) };
    assert!(!ptr1.is_null());
    assert!(!ptr2.is_null());

    // Free them in sequence so they end up in the thread-local free list
    unsafe {
        thread_free::<HardenedPolicy, MemoryBackendWrapper>(ptr1);
        thread_free::<HardenedPolicy, MemoryBackendWrapper>(ptr2);
    }

    // Now, `page.free` points to `ptr2`, and `ptr2` contains the encrypted pointer to `ptr1`.
    // Let's tamper with the encrypted next pointer in `ptr2`.
    // The block metadata stores the encrypted pointer in the first `Option<NonNull<Block>>` slot of the block.
    let val2 = ptr2 as *mut usize;
    unsafe {
        let original_val = *val2;
        // Corrupt the pointer (e.g. flip a bit in the address portion)
        *val2 = original_val ^ 0x08;
    }

    // Now, try to allocate. The first allocation gets `ptr2` (which is successful).
    let ptr3 = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(16, 8) };
    assert_eq!(ptr3, ptr2);

    // The second allocation would follow the tampered pointer to `ptr1`.
    // Since we flipped a bit, the decrypted address is incorrect and fails to match `ptr1`.
    // In particular, the page's free pointer now contains garbage.
    let ptr_val = ptr3 as usize;
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    let page = unsafe { (*segment).pages.get_unchecked(page_index) };

    let free_head = page.free.map(|p| p.as_ptr() as usize);
    assert_ne!(
        free_head,
        Some(ptr1 as usize),
        "HardenedPolicy failed to obscure/randomize the tampered pointer"
    );
}

#[test]
fn test_dealloc_path() {
    let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(1024, 8) };
    assert!(!ptr.is_null());
    unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
}

#[test]
fn test_double_free_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_DOUBLE_FREE_ABORT_TEST").is_ok() {
        unsafe {
            let ptr = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::test_double_free_aborts_process")
        .arg("--exact")
        .env("RUN_DOUBLE_FREE_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
fn test_reclaim_overflow_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_RECLAIM_OVERFLOW_ABORT_TEST").is_ok() {
        unsafe {
            let ptr = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            let ptr_val = ptr as usize;
            let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;
            let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
            let page = &mut (*segment).pages[page_index];

            // Manually reduce alloc_count to 0 so count (1) > alloc_count (0) during reclaim.
            page.set_alloc_count_for_segment(segment, page_index, 0);

            // Push the block directly to the thread_free queue.
            let block = ptr as *mut Block;
            page.thread_free
                .push::<StandardPolicy>(NonNull::new_unchecked(block));

            // Run reclaim, which should detect count (1) > alloc_count (0) and abort.
            page.reclaim_thread_free::<StandardPolicy>();
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::test_reclaim_overflow_aborts_process")
        .arg("--exact")
        .env("RUN_RECLAIM_OVERFLOW_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
fn test_cross_thread_double_free_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_CROSS_THREAD_DOUBLE_FREE_ABORT_TEST").is_ok() {
        let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8) };
        let ptr_val = ptr as usize;
        let handle = std::thread::spawn(move || unsafe {
            let ptr = ptr_val as *mut u8;
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
        });
        let _ = handle.join();
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::test_cross_thread_double_free_aborts_process")
        .arg("--exact")
        .env("RUN_CROSS_THREAD_DOUBLE_FREE_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
fn test_local_immediate_double_free_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_LOCAL_IMMEDIATE_DOUBLE_FREE_ABORT_TEST").is_ok() {
        unsafe {
            let ptr1 = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            let _ptr2 = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr1);
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr1);
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::test_local_immediate_double_free_aborts_process")
        .arg("--exact")
        .env("RUN_LOCAL_IMMEDIATE_DOUBLE_FREE_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
fn test_cpu_cache_double_free_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_CPU_CACHE_DOUBLE_FREE_ABORT_TEST").is_ok() {
        unsafe {
            crate::per_cpu::PER_CPU_CACHE_ENABLED
                .store(true, core::sync::atomic::Ordering::Relaxed);
            crate::per_cpu::enable_cpu_cache();
            let ptr = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            let ptr_val = ptr as usize;
            let handle = std::thread::spawn(move || {
                let ptr = ptr_val as *mut u8;
                thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
                thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
            });
            let _ = handle.join();
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::test_cpu_cache_double_free_aborts_process")
        .arg("--exact")
        .env("RUN_CPU_CACHE_DOUBLE_FREE_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
fn test_large_alloc_metadata_corruption_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_LARGE_ALLOC_METADATA_CORRUPTION_ABORT_TEST").is_ok() {
        unsafe {
            let ptr = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(65536, 8);
            assert!(!ptr.is_null());

            // Corrupt the metadata slot immediately preceding the payload.
            let metadata_slot = (ptr as *mut usize).sub(1);
            metadata_slot.write(0x1337); // Invalid segment pointer alignment.

            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::test_large_alloc_metadata_corruption_aborts_process")
        .arg("--exact")
        .env("RUN_LARGE_ALLOC_METADATA_CORRUPTION_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
fn test_large_alloc_segment_invariant_corruption_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_LARGE_ALLOC_SEGMENT_INVARIANT_CORRUPTION_ABORT_TEST").is_ok() {
        unsafe {
            let ptr = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(65536, 8);
            assert!(!ptr.is_null());

            // Retrieve the valid segment pointer from the metadata slot.
            let segment_ptr = *((ptr as *mut *mut Segment).sub(1));

            // Corrupt the segment header's raw_alloc_ptr to violate the alignment offset invariant.
            (*segment_ptr).raw_alloc_ptr = core::ptr::null_mut();

            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::test_large_alloc_segment_invariant_corruption_aborts_process")
        .arg("--exact")
        .env(
            "RUN_LARGE_ALLOC_SEGMENT_INVARIANT_CORRUPTION_ABORT_TEST",
            "1",
        )
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
fn test_free_list_corruption_out_of_bounds_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_FREE_LIST_CORRUPTION_OUT_OF_BOUNDS_ABORT_TEST").is_ok() {
        unsafe {
            // Allocate a small block
            let ptr = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            assert!(!ptr.is_null());

            // Get containing segment and page
            let ptr_val = ptr as usize;
            let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;
            let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

            // Free the block to put it on the free list
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);

            // Corrupt the next pointer inside the block.
            // Since it's on page.free, we can decrypt/encrypt the next pointer.
            // Let's write an out-of-bounds address (e.g., 0x12345678) as the next pointer.
            let cookie = (*segment).keys[page_index];
            let corrupt_block = ptr as *mut Block;
            // Write corrupted next pointer
            let bad_ptr = 0x12345678 as *mut Block;
            (*corrupt_block).set_next::<StandardPolicy>(NonNull::new(bad_ptr), cookie);

            // Allocate again. This should pop the corrupted next pointer, but
            // when pop_block retrieves it from page.free, it will validate it and abort!
            let _ptr_new = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            let _ptr_another = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::test_free_list_corruption_out_of_bounds_aborts_process")
        .arg("--exact")
        .env("RUN_FREE_LIST_CORRUPTION_OUT_OF_BOUNDS_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}
