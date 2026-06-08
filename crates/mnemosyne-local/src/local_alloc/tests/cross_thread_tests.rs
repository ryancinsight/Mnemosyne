use super::super::*;
use crate::LocalAllocatorSelector;
use core::ptr::NonNull;
use mnemosyne_arena::{allocate_segment, deallocate_segment};
use mnemosyne_core::constants::{PAGES_PER_SEGMENT, PAGE_SHIFT};
use mnemosyne_core::policy::StandardPolicy;

#[test]
fn test_snmalloc_message_passing() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use std::thread;

    // Purge global segment pool to ensure we must allocate from the OS.
    unsafe {
        mnemosyne_arena::purge_segment_pool::<DefaultBackend>();
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }

    let mut alloc_a = ThreadAllocator::<DefaultBackend>::new();
    // Safety: alloc_a is initialized and valid.
    let ptr = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
    assert!(
        !ptr.is_null(),
        "producer allocation for cross-thread free failed"
    );

    let ptr_usize = ptr as usize;

    // Verify that another thread can free A's block through the owning page queue.
    let handle = thread::spawn(move || {
        // Safety: freeing block allocated by A
        unsafe {
            crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(
                ptr_usize as *mut u8,
            );
        }
    });
    handle.join().expect("cross-thread free worker panicked");

    let mut reclaimed_remote_free = false;
    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    let max_blocks = unsafe { (*segment).pages[page_index].max_blocks() };
    for _ in 0..max_blocks {
        // Safety: alloc_a is valid.
        let ptr2 = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
        assert!(
            !ptr2.is_null(),
            "reclaim probe allocation failed before reclaiming remote free"
        );
        if ptr2 == ptr {
            reclaimed_remote_free = true;
            break;
        }
    }

    assert!(
        reclaimed_remote_free,
        "cross-thread freed block was not reclaimed after {} small allocations",
        max_blocks
    );
}

#[test]
fn cross_thread_free_does_not_charge_non_owner_defrag_counter() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use std::thread;

    let mut owner = ThreadAllocator::<DefaultBackend>::new();
    // Safety: owner is initialized and valid.
    let ptr = unsafe { owner.alloc::<StandardPolicy>(32) };
    assert!(
        !ptr.is_null(),
        "producer allocation for cross-thread free failed"
    );

    let ptr_usize = ptr as usize;
    let handle = thread::spawn(move || {
        DefaultBackend::with_allocator(|alloc| {
            assert_eq!(alloc.defrag_counter, 0);
        })
        .expect("worker allocator slot unavailable before remote free");

        // Safety: freeing block allocated by owner; this thread does not own
        // the target page and must only enqueue it for owner-side reclamation.
        unsafe {
            crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(
                ptr_usize as *mut u8,
            );
        }

        DefaultBackend::with_allocator(|alloc| {
            assert_eq!(
                alloc.defrag_counter, 0,
                "remote free charged defrag work to the non-owner allocator"
            );
        })
        .expect("worker allocator slot unavailable after remote free");
    });
    handle.join().expect("cross-thread free worker panicked");

    let mut reclaimed_remote_free = false;
    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    let max_blocks = unsafe { (*segment).pages[page_index].max_blocks() };
    for _ in 0..max_blocks {
        // Safety: owner is valid.
        let ptr2 = unsafe { owner.alloc::<StandardPolicy>(32) };
        assert!(
            !ptr2.is_null(),
            "reclaim probe allocation failed before reclaiming remote free"
        );
        if ptr2 == ptr {
            reclaimed_remote_free = true;
            break;
        }
    }

    assert!(
        reclaimed_remote_free,
        "cross-thread freed block was not reclaimed after {} small allocations",
        max_blocks
    );
}

#[test]
fn test_orphan_segment_reuse() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use std::sync::mpsc;
    use std::thread;

    unsafe {
        mnemosyne_arena::purge_segment_pool::<DefaultBackend>();
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }

    let (tx, rx) = mpsc::channel();

    // Thread A allocates a block and exits
    thread::spawn(move || {
        let mut alloc_a = ThreadAllocator::<DefaultBackend>::new();
        // Safety: alloc_a is valid.
        let ptr = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
        assert!(!ptr.is_null(), "orphan producer allocation failed");
        tx.send(ptr as usize)
            .expect("orphan producer failed to send live allocation pointer");
    })
    .join()
    .expect("orphan producer thread panicked");

    let live_ptr =
        rx.recv()
            .expect("orphan producer did not send live allocation pointer") as *mut u8;

    // Thread B allocates a block. It should reuse the orphaned segment from A!
    let mut alloc_b = ThreadAllocator::<DefaultBackend>::new();
    // Safety: alloc_b is valid.
    let ptr_b = unsafe { alloc_b.alloc::<StandardPolicy>(64) };
    assert!(!ptr_b.is_null(), "orphan consumer allocation failed");

    // Assert that B reused the orphaned segment: current owned segments must be 1, not 2!
    assert_eq!(alloc_b.stats().current_thread_owned_segments, 1);

    // Free the allocations
    // Safety: pointers are valid and exclusive.
    unsafe {
        crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(live_ptr);
        crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(ptr_b);
    }
}

#[test]
fn test_online_defragmentation_page_prioritization() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();

    // Allocate two segments
    let seg1 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg1 allocation failed");
    let seg2 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg2 allocation failed");

    // Make seg1 dirty by setting alloc_count on page 1
    unsafe {
        (*seg1).pages[1].set_alloc_count(1);
        (*seg1).pages[2].set_alloc_count(0);
    }

    // Make seg2 clean by setting alloc_count on all pages to 0
    unsafe {
        for i in 1..mnemosyne_core::constants::PAGES_PER_SEGMENT {
            (*seg2).pages[i].set_alloc_count(0);
        }
    }

    let seg1_page2 = unsafe { NonNull::new_unchecked(&mut (*seg1).pages[2] as *mut Page) };
    let seg2_page1 = unsafe { NonNull::new_unchecked(&mut (*seg2).pages[1] as *mut Page) };

    // Push seg1_page2 first, then seg2_page1 second
    unsafe {
        alloc.push_empty_page(seg1_page2);
        alloc.push_empty_page(seg2_page1);
    }

    // pop_best_empty_page should prioritize the page in seg1 (the dirty segment)
    let popped = unsafe { alloc.pop_best_empty_page() };
    assert_eq!(popped, Some(seg1_page2));

    // The second call should fall back to the clean segment page
    let popped2 = unsafe { alloc.pop_best_empty_page() };
    assert_eq!(popped2, Some(seg2_page1));

    // A third call should return None
    let popped3 = unsafe { alloc.pop_best_empty_page() };
    assert_eq!(popped3, None);

    // Clean up
    unsafe {
        deallocate_segment::<DefaultBackend>(seg1);
        deallocate_segment::<DefaultBackend>(seg2);
    }
}

#[test]
fn test_periodic_defragmentation_segment_reclaim() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    // Case 1: Count < 4. Empty segments should be retained.
    {
        let mut alloc = ThreadAllocator::<DefaultBackend>::new();
        let seg1 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg1 failed");
        let seg2 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg2 failed");
        let seg3 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg3 failed");

        unsafe {
            alloc.push_owned_segment::<StandardPolicy>(seg1);
            alloc.push_owned_segment::<StandardPolicy>(seg2);
            alloc.push_owned_segment::<StandardPolicy>(seg3);
        }

        // Verify we have 3 segments
        let stats = alloc.stats();
        assert_eq!(stats.current_thread_owned_segments, 3);

        // Run sweep
        unsafe {
            alloc.periodic_defragmentation_sweep::<StandardPolicy>();
        }

        // Verify we still have 3 segments (none reclaimed because count < 4)
        let stats = alloc.stats();
        assert_eq!(stats.current_thread_owned_segments, 3);
    }

    // Case 2: Count >= 4. Empty segments should be reclaimed down to 3.
    {
        let mut alloc = ThreadAllocator::<DefaultBackend>::new();
        let seg1 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg1 failed");
        let seg2 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg2 failed");
        let seg3 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg3 failed");
        let seg4 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg4 failed");

        unsafe {
            alloc.push_owned_segment::<StandardPolicy>(seg1);
            alloc.push_owned_segment::<StandardPolicy>(seg2);
            alloc.push_owned_segment::<StandardPolicy>(seg3);
            alloc.push_owned_segment::<StandardPolicy>(seg4);
        }

        // Set seg1 as the current active segment
        unsafe {
            alloc.set_current_segment(Some(NonNull::new_unchecked(seg1)));
        }

        // Verify we have 4 segments
        let stats = alloc.stats();
        assert_eq!(stats.current_thread_owned_segments, 4);

        // Run sweep
        unsafe {
            alloc.periodic_defragmentation_sweep::<StandardPolicy>();
        }

        // Verify that one segment (seg4, which is head of list, or one of the empty ones)
        // was reclaimed, leaving exactly 3 segments.
        let stats = alloc.stats();
        assert_eq!(stats.current_thread_owned_segments, 3);

        // Verify that seg1 (current active segment) was not reclaimed
        assert!(alloc.is_current_segment(seg1));
    }
}
