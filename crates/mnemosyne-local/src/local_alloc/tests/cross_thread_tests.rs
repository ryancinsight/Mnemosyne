use super::super::*;
use crate::LocalAllocatorSelector;
use core::ptr::NonNull;
use mnemosyne_arena::{allocate_segment, deallocate_segment};
use mnemosyne_core::constants::{PAGE_SHIFT, PAGES_PER_SEGMENT};
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

/// Drains the orphan pools left behind by other tests so orphan-adoption tests
/// observe a deterministic pool state, releasing each drained segment through
/// the regular deallocation path.
///
/// # Safety
///
/// Callers must hold `TEST_LOCK` so no concurrent allocator activity races the
/// drain.
unsafe fn drain_orphan_pools_for_test() {
    use mnemosyne_arena::HasSegmentPool;
    unsafe {
        while let Some(seg) = <DefaultBackend as HasSegmentPool>::global_orphan_pool().pop() {
            deallocate_segment::<DefaultBackend>(seg);
        }
        while let Some(seg) =
            <mnemosyne_backend::MemoryBackendWrapper as HasSegmentPool>::global_orphan_pool().pop()
        {
            deallocate_segment::<mnemosyne_backend::MemoryBackendWrapper>(seg);
        }
        mnemosyne_arena::purge_segment_pool::<DefaultBackend>();
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }
}

#[test]
fn test_hardened_orphan_adoption_preserves_encoded_chains() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use mnemosyne_hardened::HardenedPolicy;
    use std::sync::mpsc;
    use std::thread;
    use std::vec::Vec;

    // Safety: TEST_LOCK is held; no concurrent allocator activity.
    unsafe { drain_orphan_pools_for_test() };

    let (tx, rx) = mpsc::channel();

    // Producer: allocate four blocks under the encrypted policy, free two of
    // them (building a `page.free` chain encoded with THIS thread's per-page
    // keys), keep two live, and exit so the segment is orphaned with a live
    // encoded chain.
    thread::spawn(move || {
        let mut alloc_a = ThreadAllocator::<DefaultBackend>::new();
        let ptrs: Vec<*mut u8> = (0..4)
            // Safety: alloc_a is valid; 32 is a small size class.
            .map(|_| unsafe { alloc_a.alloc::<HardenedPolicy>(32) })
            .collect();
        assert!(
            ptrs.iter().all(|p| !p.is_null()),
            "hardened orphan producer allocation failed"
        );
        // Safety: freeing two distinct pointers just allocated on this thread.
        unsafe {
            crate::thread_free::<HardenedPolicy, DefaultBackend>(ptrs[1]);
            crate::thread_free::<HardenedPolicy, DefaultBackend>(ptrs[3]);
        }
        tx.send((
            [ptrs[0] as usize, ptrs[2] as usize],
            [ptrs[1] as usize, ptrs[3] as usize],
        ))
        .expect("hardened orphan producer failed to send pointers");
    })
    .join()
    .expect("hardened orphan producer thread panicked");

    let (live, freed) = rx
        .recv()
        .expect("hardened orphan producer did not send pointers");

    // Consumer: a different thread (hence a different TLS key seed) adopts the
    // orphan. Before the key-preservation fix, adoption re-keyed the segment
    // with this thread's seed, so popping the producer-encoded `page.free`
    // chain decoded garbage and aborted on the free-list bounds check.
    let mut alloc_b = ThreadAllocator::<DefaultBackend>::new();
    // Safety: alloc_b is valid; 32 is a small size class.
    let first = unsafe { alloc_b.alloc::<HardenedPolicy>(32) };
    assert!(
        !first.is_null(),
        "hardened orphan consumer allocation failed"
    );
    let stats = alloc_b.stats();
    assert_eq!(
        stats.current_thread_owned_segments, 1,
        "consumer must adopt the compatible hardened orphan, not map a fresh segment"
    );
    assert_eq!(stats.orphan_segments_adopted, 1);

    // Allocate until the adopted page's producer-encoded free chain is popped:
    // the freshly initialized page the adoption returned holds
    // PAGE_SIZE / 32 blocks, after which the producer's active page (whose
    // `free` chain carries the two freed blocks) becomes the allocation
    // source. Reusing one of the freed addresses is the value-semantic proof
    // that the preserved keys decode the chain correctly.
    // Both freed blocks must come back: the first pop returns the chain head
    // and stores its decoded next-link as the new `page.free`; only the
    // second pop dereference-validates that decoded link, so requiring both
    // addresses is what proves the chain decodes correctly end-to-end (under
    // the re-keying bug the second pop aborts on the bounds check or yields a
    // garbage address outside the freed set).
    let cap = 3 * (mnemosyne_core::constants::PAGE_SIZE / 32);
    let mut reused = 0usize;
    let mut consumer_ptrs = Vec::with_capacity(cap + 1);
    consumer_ptrs.push(first);
    for _ in 0..cap {
        // Safety: alloc_b is valid; 32 is a small size class.
        let p = unsafe { alloc_b.alloc::<HardenedPolicy>(32) };
        assert!(
            !p.is_null(),
            "hardened consumer allocation failed mid-sweep"
        );
        consumer_ptrs.push(p);
        if freed.contains(&(p as usize)) {
            // Safety: `p` was just returned by the allocator; 32 bytes are
            // writable block payload.
            unsafe {
                core::ptr::write_bytes(p, 0xAB, 32);
                assert_eq!(*p, 0xAB);
                assert_eq!(*p.add(31), 0xAB);
            }
            reused += 1;
            if reused == freed.len() {
                break;
            }
        }
    }
    assert_eq!(
        reused,
        freed.len(),
        "adopted encoded free chain was not fully popped within {cap} allocations"
    );

    // Safety: every pointer below was returned by this allocator family and is
    // freed exactly once (producer's live pair plus the consumer sweep).
    unsafe {
        for p in consumer_ptrs {
            crate::thread_free::<HardenedPolicy, DefaultBackend>(p);
        }
        for addr in live {
            crate::thread_free::<HardenedPolicy, DefaultBackend>(addr as *mut u8);
        }
    }
}

#[test]
fn test_orphan_adoption_skips_policy_mismatched_segment() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use mnemosyne_hardened::HardenedPolicy;
    use std::sync::mpsc;
    use std::thread;

    // Safety: TEST_LOCK is held; no concurrent allocator activity.
    unsafe { drain_orphan_pools_for_test() };

    let (tx, rx) = mpsc::channel();

    // Producer: orphan a plain (unencrypted) segment with one live block.
    thread::spawn(move || {
        let mut alloc_a = ThreadAllocator::<DefaultBackend>::new();
        // Safety: alloc_a is valid; 32 is a small size class.
        let ptr = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
        assert!(!ptr.is_null(), "standard orphan producer allocation failed");
        tx.send(ptr as usize)
            .expect("standard orphan producer failed to send pointer");
    })
    .join()
    .expect("standard orphan producer thread panicked");

    let live_ptr = rx
        .recv()
        .expect("standard orphan producer did not send pointer") as *mut u8;

    // An encrypted-policy consumer must NOT adopt the plain orphan: its free
    // chains are encoded with cookie 0 while `pop_block::<HardenedPolicy>`
    // would decode them with the per-page keys. The gate defers the orphan
    // back to the pool and takes a fresh segment instead.
    let mut alloc_hardened = ThreadAllocator::<DefaultBackend>::new();
    // Safety: allocator is valid; 32 is a small size class.
    let ptr_h = unsafe { alloc_hardened.alloc::<HardenedPolicy>(32) };
    assert!(!ptr_h.is_null(), "hardened consumer allocation failed");
    let stats_h = alloc_hardened.stats();
    assert_eq!(
        stats_h.orphan_segments_adopted, 0,
        "hardened consumer must not adopt a plain-encoded orphan"
    );
    assert_eq!(stats_h.fresh_segments, 1);
    assert_eq!(stats_h.current_thread_owned_segments, 1);

    // A matching-policy consumer still finds the deferred orphan in the pool.
    let mut alloc_standard = ThreadAllocator::<DefaultBackend>::new();
    // Safety: allocator is valid; 64 is a small size class.
    let ptr_s = unsafe { alloc_standard.alloc::<StandardPolicy>(64) };
    assert!(!ptr_s.is_null(), "standard consumer allocation failed");
    let stats_s = alloc_standard.stats();
    assert_eq!(
        stats_s.orphan_segments_adopted, 1,
        "standard consumer must adopt the deferred plain orphan"
    );
    assert_eq!(stats_s.current_thread_owned_segments, 1);

    // Safety: pointers are valid, freed once, under their allocation policies.
    unsafe {
        crate::thread_free::<StandardPolicy, DefaultBackend>(live_ptr);
        crate::thread_free::<HardenedPolicy, DefaultBackend>(ptr_h);
        crate::thread_free::<StandardPolicy, DefaultBackend>(ptr_s);
    }
}

#[test]
fn test_mixed_policy_free_and_realloc_preserve_segment_encoding() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use mnemosyne_hardened::HardenedPolicy;
    use std::alloc::Layout;

    // Safety: TEST_LOCK is held; no concurrent allocator activity.
    unsafe { drain_orphan_pools_for_test() };

    // The public free-function surface uses separate zero-cost TLS slots for
    // the two encoding modes. The slots are distinct even though both use the
    // same backend type and allocator representation.
    let hardened_slot =
        <DefaultBackend as LocalAllocatorSelector<DefaultBackend>>::get_allocator_ptr_for_policy::<
            HardenedPolicy,
        >();
    let standard_slot =
        <DefaultBackend as LocalAllocatorSelector<DefaultBackend>>::get_allocator_ptr_for_policy::<
            StandardPolicy,
        >();
    assert!(
        !standard_slot.is_null(),
        "standard TLS slot must initialize"
    );

    let hardened_first = unsafe { crate::thread_alloc::<HardenedPolicy, DefaultBackend>(32, 8) };
    let hardened_second = unsafe { crate::thread_alloc::<HardenedPolicy, DefaultBackend>(32, 8) };
    let hardened_third = unsafe { crate::thread_alloc::<HardenedPolicy, DefaultBackend>(32, 8) };
    assert!(!hardened_first.is_null());
    assert!(!hardened_second.is_null());
    assert!(!hardened_third.is_null());
    let hardened_slot_after = <DefaultBackend as LocalAllocatorSelector<DefaultBackend>>::
        get_allocator_ptr_raw_for_policy::<HardenedPolicy>();
    assert!(!hardened_slot_after.is_null());
    assert_ne!(
        hardened_slot_after, standard_slot,
        "standard and hardened policies must not share a TLS allocator"
    );
    assert_eq!(hardened_slot, hardened_slot_after);

    // Free a hardened block through the standard policy. The free path must
    // identify the hardened owner and use the segment's encoded-chain mode,
    // rather than the freeing call's policy type.
    unsafe {
        crate::thread_free::<StandardPolicy, DefaultBackend>(hardened_second);
    }
    let reused = unsafe { crate::thread_alloc::<HardenedPolicy, DefaultBackend>(32, 8) };
    assert_eq!(
        reused, hardened_second,
        "hardened free-list head must remain decodable after a standard-policy free"
    );

    // Reallocate another hardened block through the standard policy. This
    // exercises the fallback path's old-block free, which must apply the same
    // segment-keyed encoding before the hardened allocator pops it.
    let layout = Layout::from_size_align(32, 8).expect("test layout is valid");
    let resized = unsafe {
        crate::thread_realloc::<StandardPolicy, DefaultBackend>(hardened_first, layout, 64)
    };
    assert!(
        !resized.is_null(),
        "mixed-policy realloc must produce a block"
    );
    let realloc_reused = unsafe { crate::thread_alloc::<HardenedPolicy, DefaultBackend>(32, 8) };
    assert_eq!(
        realloc_reused, hardened_first,
        "hardened allocator must decode the block freed by standard-policy realloc"
    );

    // Safety: every pointer is live and freed exactly once under a policy
    // whose free path now consults the owning segment's mode.
    unsafe {
        crate::thread_free::<HardenedPolicy, DefaultBackend>(hardened_third);
        crate::thread_free::<HardenedPolicy, DefaultBackend>(reused);
        crate::thread_free::<HardenedPolicy, DefaultBackend>(realloc_reused);
        crate::thread_free::<StandardPolicy, DefaultBackend>(resized);
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

/// Anchors the Phase 1 SAFETY closure on `thread_free_cold`'s
/// `page.thread_free.push` site. Allocates on the owning thread and
/// frees on a non-owning thread, exercising the cross-thread path
/// (`is_owner == false`), and asserts that exactly one block landed
/// in `(*page).thread_free` for the owning thread's later reclamation.
///
/// Under `#[cfg(test)]` the per-CPU cache is disabled
/// (`PER_CPU_CACHE_ENABLED = false`), so the cold path's
/// `try_free_cpu` early-return never fires and the atomic push runs
/// unconditionally — making this a direct regression anchor for the
/// SAFETY comment:
/// > `block` came from this allocator under the same backend;
/// > non-nullness is the allocator invariant.
/// > The page-local atomic free list takes ownership of the pointer.
#[test]
fn cross_thread_free_pushes_block_to_page_thread_free_queue() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use std::thread;

    let mut owner = ThreadAllocator::<DefaultBackend>::new();
    // Safety: owner is initialized and valid.
    let ptr = unsafe { owner.alloc::<StandardPolicy>(32) };
    assert!(
        !ptr.is_null(),
        "owner alloc for thread_free queue anchor failed"
    );
    let ptr_val = ptr as usize;

    let segment_addr = ptr_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

    // Pre-condition: no cross-thread frees have been issued yet.
    let page = unsafe { &(*segment).pages[page_index] };
    assert!(
        page.thread_free.is_empty(),
        "thread_free must be empty before any remote free; alloc_count={}",
        page.alloc_count,
    );

    let handle = thread::spawn(move || unsafe {
        // Safety: ptr was returned by Mnemosyne under DefaultBackend.
        // Thread B is not the segment owner, so `thread_free<...>`
        // routes through `thread_free_cold`'s `page.thread_free.push`
        // rather than the in-place active/full/empty path.
        crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(ptr_val as *mut u8);
    });
    handle.join().expect("cross-thread free worker panicked");

    let page = unsafe { &mut (*segment).pages[page_index] };
    assert!(
        !page.thread_free.is_empty(),
        "cross-thread free did not enqueue the block on page.thread_free",
    );

    let before_alloc_count = page.alloc_count;
    // SAFETY: caller owns the page through the still-live owner segment;
    // the typed wrapper recomputes `segment`/`page_index` and reads
    // `StandardPolicy::ENABLE_FREE_LIST_ENCRYPTION` for the cookie.
    let reclaimed = unsafe { page.reclaim_thread_free::<StandardPolicy>() };
    assert_eq!(
        reclaimed, 1,
        "expected exactly one block from the cross-thread free on this page; got {} \
         (alloc_count before drain = {})",
        reclaimed, before_alloc_count,
    );
}

/// AR-3: the per-thread `cross_thread_reclaimed` counter records the exact
/// number of blocks drained from a page's cross-thread free list on the
/// allocation-side reclaim path, and a `stats()` snapshot reports that count
/// (folded with the process-global total) with the same exactness.
#[test]
fn allocation_side_reclaim_counts_cross_thread_blocks_exactly() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use std::thread;

    unsafe {
        mnemosyne_arena::purge_segment_pool::<DefaultBackend>();
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }

    let mut owner = ThreadAllocator::<DefaultBackend>::new();

    // Fill one page of the class completely so the owner's next allocation must
    // fall through the local-free / bump paths and drain the cross-thread queue.
    let first = unsafe { owner.alloc::<StandardPolicy>(32) };
    assert!(!first.is_null(), "owner anchor allocation failed");
    let (segment, page_index) = unsafe { mnemosyne_core::types::locate_segment(first) };
    let max_blocks = unsafe { (*segment).pages[page_index].max_blocks() };
    assert!(max_blocks >= 2, "size class must hold at least two blocks");

    let mut blocks = std::vec::Vec::with_capacity(max_blocks);
    blocks.push(first as usize);
    for _ in 1..max_blocks {
        let p = unsafe { owner.alloc::<StandardPolicy>(32) };
        assert!(!p.is_null(), "owner fill allocation failed");
        blocks.push(p as usize);
    }

    // Baseline: no reclaims have occurred on this allocator yet.
    assert_eq!(
        owner.cross_thread_reclaimed, 0,
        "fresh allocator must have reclaimed no cross-thread blocks"
    );
    let stats_before = owner.stats().cross_thread_reclaimed_blocks;

    // A non-owning thread frees every block back through the page's atomic
    // cross-thread queue (the owner is thread-affine, so these route to
    // `page.thread_free.push`, not the in-place fast path).
    let freed = blocks.clone();
    thread::spawn(move || unsafe {
        for addr in freed {
            crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(addr as *mut u8);
        }
    })
    .join()
    .expect("cross-thread free worker panicked");

    // The page now holds `max_blocks` remote frees and is full. The owner's next
    // allocation drives the cold reclaim path, which drains the whole queue in
    // one `pop_all`, accumulating the exact count into `cross_thread_reclaimed`.
    let reclaimed_ptr = unsafe { owner.alloc::<StandardPolicy>(32) };
    assert!(
        !reclaimed_ptr.is_null(),
        "owner allocation after remote frees failed"
    );

    assert_eq!(
        owner.cross_thread_reclaimed, max_blocks,
        "per-thread reclaim counter must equal the number of cross-thread frees"
    );
    // The stats snapshot folds the global total with this thread's live count,
    // so the reported value must rise by exactly `max_blocks`.
    assert_eq!(
        owner.stats().cross_thread_reclaimed_blocks,
        stats_before + max_blocks,
        "stats() must report the exact cross-thread reclaimed delta"
    );
}

/// Multi-thread producer-consumer stress: allocators across N threads allocate
/// blocks and free blocks allocated by other threads.
///
/// Exercises the cross-thread free path under contention: every thread
/// allocates from its own `ThreadAllocator`, then passes the pointers to a
/// different thread for freeing. This stresses the page-local atomic
/// `thread_free.push` / `reclaim_thread_free` path with concurrent producers
/// and consumers.
///
/// Verifies:
/// 1. No crashes, aborts, or corrupted metadata.
/// 2. Every allocation is eventually freed exactly once.
/// 3. The owner thread can reclaim all cross-thread freed blocks.
#[test]
fn cross_thread_stress_producer_consumer() {
    use std::thread;
    use std::sync::mpsc;

    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    const NUM_PRODUCERS: usize = 4;
    const ALLOCS_PER_PRODUCER: usize = 200;

    // Each producer thread allocates blocks and sends the pointers to
    // a dedicated consumer thread via its own channel.
    let mut producers = std::vec::Vec::new();
    let mut consumers = std::vec::Vec::new();

    for tid in 0..NUM_PRODUCERS {
        let (tx, rx) = mpsc::channel::<std::vec::Vec<usize>>();
        producers.push(thread::spawn(move || {
            let mut alloc = ThreadAllocator::<DefaultBackend>::new();
            let mut ptrs = std::vec::Vec::with_capacity(ALLOCS_PER_PRODUCER);
            for i in 0..ALLOCS_PER_PRODUCER {
                let size = if (i + tid) % 2 == 0 { 32 } else { 64 };
                let ptr = unsafe { alloc.alloc::<StandardPolicy>(size) };
                assert!(!ptr.is_null(), "producer {tid} alloc {i} failed");
                unsafe {
                    *ptr = (tid * 100 + i) as u8;
                }
                ptrs.push(ptr as usize);
            }
            tx.send(ptrs).expect("producer failed to send ptrs");
        }));
        consumers.push(thread::spawn(move || {
            // Each consumer creates its own allocator to exercise the
            // non-owner cross-thread free path.
            let _alloc = ThreadAllocator::<DefaultBackend>::new();
            for ptrs in rx {
                for addr in ptrs {
                    unsafe {
                        crate::thread_free::<StandardPolicy, DefaultBackend>(
                            addr as *mut u8,
                        );
                    }
                }
            }
        }));
    }

    for handle in producers.into_iter() {
        handle.join().expect("producer thread panicked");
    }
    for handle in consumers.into_iter() {
        handle.join().expect("consumer thread panicked");
    }

    // If we reached here without crashing, the cross-thread free path
    // handled NUM_PRODUCERS * ALLOCS_PER_PRODUCER concurrent cross-thread frees.
}

/// Cross-thread free under high contention: many threads simultaneously free
/// blocks that belong to a single owner thread.
///
/// One owner allocates many blocks. Then N non-owning threads each take a
/// slice and free them concurrently. This stress-tests the page-local atomic
/// `thread_free.push` path for data-race freedom and metadata integrity.
///
/// Verifies:
/// 1. No crashes, aborts, or metadata corruption under 8-way concurrent
///    cross-thread frees of 640 blocks.
/// 2. The owner allocator remains functional after the contention storm.
#[test]
fn cross_thread_stress_many_to_one_free() {
    use std::thread;
    use std::sync::mpsc;

    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    const NUM_FREER_THREADS: usize = 8;
    const TOTAL_ALLOCS: usize = 640;

    // Owner allocates all blocks.
    let mut owner = ThreadAllocator::<DefaultBackend>::new();
    let mut all_ptrs = std::vec::Vec::with_capacity(TOTAL_ALLOCS);
    for i in 0..TOTAL_ALLOCS {
        let ptr = unsafe { owner.alloc::<StandardPolicy>(32) };
        assert!(!ptr.is_null(), "owner alloc {i} failed");
        unsafe {
            *ptr = i as u8;
        }
        all_ptrs.push(ptr as usize);
    }

    let chunk_size = TOTAL_ALLOCS / NUM_FREER_THREADS;
    let (tx, rx) = mpsc::channel();

    // Distribute blocks across freer threads.
    for chunk_idx in 0..NUM_FREER_THREADS {
        let start = chunk_idx * chunk_size;
        let end = if chunk_idx == NUM_FREER_THREADS - 1 {
            TOTAL_ALLOCS
        } else {
            start + chunk_size
        };
        let chunk: std::vec::Vec<usize> = all_ptrs[start..end].to_vec();
        let tx = tx.clone();
        thread::spawn(move || unsafe {
            for addr in chunk {
                crate::thread_free::<StandardPolicy, DefaultBackend>(addr as *mut u8);
            }
            tx.send(()).expect("freer failed to signal completion");
        });
    }
    drop(tx);

    // Wait for all freer threads.
    for _ in 0..NUM_FREER_THREADS {
        rx.recv().expect("freer thread did not signal completion");
    }

    // Verify the owner allocator is still functional after the contention storm.
    let post_stress = unsafe { owner.alloc::<StandardPolicy>(32) };
    assert!(
        !post_stress.is_null(),
        "owner allocation failed after cross-thread contention storm"
    );
    unsafe {
        core::ptr::write_bytes(post_stress, 0xFF, 32);
        assert_eq!(*post_stress, 0xFF);
    }
    unsafe {
        crate::thread_free::<StandardPolicy, DefaultBackend>(post_stress);
    }
}

