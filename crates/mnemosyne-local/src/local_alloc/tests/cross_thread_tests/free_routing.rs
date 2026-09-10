//! Cross-thread free routing into the owning page's queue.

use super::*;

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
    // SAFETY: alloc_a is initialized and valid.
    let ptr = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
    assert!(
        !ptr.is_null(),
        "producer allocation for cross-thread free failed"
    );

    let ptr_usize = ptr as usize;

    // Verify that another thread can free A's block through the owning page queue.
    let handle = thread::spawn(move || {
        // SAFETY: freeing block allocated by A
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
    let mut probe_allocations = std::vec::Vec::with_capacity(max_blocks);
    for _ in 0..max_blocks {
        // SAFETY: alloc_a is valid.
        let ptr2 = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
        assert!(
            !ptr2.is_null(),
            "reclaim probe allocation failed before reclaiming remote free"
        );
        probe_allocations.push(ptr2);
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

    // The probe owns every address it observed, including the reclaimed one.
    // Release the complete set so allocator teardown tests pool retention
    // rather than treating the intentionally retained probes as live memory.
    unsafe {
        for probe in probe_allocations {
            crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(probe);
        }
    }
    alloc_a.reclaim_owned_segments();
}

#[test]
fn test_mixed_policy_free_and_realloc_preserve_segment_encoding() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use mnemosyne_core::policy::HardenedPolicy;
    use std::alloc::Layout;

    // SAFETY: TEST_LOCK is held; no concurrent allocator activity.
    unsafe { super::super::fixtures::drain_all_pools() };

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
    // The block must be reachable in its owner page's chain -- which is the
    // decodability claim -- not at any particular position in it. Probing by
    // reallocation until the same address comes back asserts reuse identity
    // instead, and the randomized head (ADR 0001, revised 2026-09-09) makes
    // that a coin flip whose odds depend on the page geometry: the probe form
    // passed on x86-64 and failed on aarch64.
    assert!(
        hardened_chain_contains(hardened_second),
        "a standard-policy free must leave the hardened block decodable in its owner chain"
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
    // Same claim for the realloc path's old-block free: reachable in the
    // chain, not necessarily at its head.
    assert!(
        hardened_chain_contains(hardened_first),
        "the block freed by a standard-policy realloc must stay decodable in the owner chain"
    );

    // SAFETY: every pointer is live and freed exactly once under a policy
    // whose free path now consults the owning segment's mode.
    unsafe {
        crate::thread_free::<HardenedPolicy, DefaultBackend>(hardened_third);
        crate::thread_free::<StandardPolicy, DefaultBackend>(resized);
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
    // SAFETY: owner is initialized and valid.
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
        // SAFETY: ptr was returned by Mnemosyne under DefaultBackend.
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
    // the typed wrapper uses the existing segment mapping and reads
    // `StandardPolicy::ENABLE_FREE_LIST_ENCRYPTION` for the cookie.
    let reclaimed =
        unsafe { Page::reclaim_thread_free_for_policy::<StandardPolicy>(segment, page_index) };
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

    unsafe {
        crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(reclaimed_ptr);
    }
    owner.reclaim_owned_segments();
}

/// Whether `block` is reachable by decoding its owner page's free chains.
///
/// Both lists are walked under the page's own hardened cookie, bounded by the
/// page's block count so a corrupted link ends the walk instead of spinning.
/// A link encoded with the wrong cookie decodes to a wild address, which is
/// exactly what fails to match.
fn hardened_chain_contains(block: *mut u8) -> bool {
    use mnemosyne_core::policy::HardenedPolicy;

    let value = block as usize;
    let segment = (value & !(mnemosyne_core::constants::SEGMENT_SIZE - 1)) as *mut Segment;
    let page_index = (value >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    // SAFETY: `block` is a live allocation, so its segment is mapped and
    // `page_index` names one of its pages.
    let page = unsafe { &raw mut (*segment).pages[page_index] };
    let cookie = unsafe { Segment::cookie_for::<HardenedPolicy>(segment, page_index) };
    let bound = unsafe { (*page).max_blocks() };
    let target = block.cast::<Block>();

    // SAFETY: each `current` is a block of this page, reached by decoding the
    // previous link with the page's own cookie.
    for head in unsafe { [(*page).free, (*page).secondary_free] } {
        let mut current = head;
        for _ in 0..bound {
            let Some(node) = current else { break };
            if node.as_ptr() == target {
                return true;
            }
            current = unsafe { (*node.as_ptr()).get_next::<HardenedPolicy>(cookie) };
        }
    }
    false
}
