//! Free helpers the sibling crates call with an allocator already in hand.

use crate::ThreadAllocator;
use crate::free_helpers::is_sole_active_page;
use crate::local_alloc::page::{
    move_page_between_lists_branded, push_page_front, unlink_page_from_list, with_page_list_token,
};
use core::ptr::NonNull;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::types::{Block, Page, Segment};

/// Internal implementation of local deallocation.
///
/// # Safety
///
/// The block pointer must point to a valid block allocated in the target page and segment.
#[inline(always)]
pub unsafe fn do_local_free_internal<B: HasSegmentPool>(
    alloc: &mut ThreadAllocator<B>,
    block: *mut Block,
    page: *mut Page,
    segment: *mut Segment,
    page_index: usize,
) -> bool {
    // SAFETY: forwards this function's own contract unchanged — the caller has
    // established that `block` is a valid block allocated in `page` within
    // `segment`, which is exactly what the policy-aware inner call requires.
    unsafe {
        do_local_free_internal_policy::<mnemosyne_core::policy::StandardPolicy, B>(
            alloc, block, page, segment, page_index,
        )
    }
}

/// Policy-aware inner free — generic over `P` so `DELAY_PAGE_WAKE` and other
/// compile-time flags can be propagated without runtime overhead.
///
/// # Safety
///
/// Same contract as `do_local_free_internal`.
#[inline(always)]
pub unsafe fn do_local_free_internal_policy<
    P: mnemosyne_core::policy::AllocPolicy,
    B: HasSegmentPool,
>(
    alloc: &mut ThreadAllocator<B>,
    block: *mut Block,
    page: *mut Page,
    segment: *mut Segment,
    page_index: usize,
) -> bool {
    // SAFETY: `page` is valid per this function's contract, and the caller holds
    // the owner's exclusive page-list access, so `alloc_count` has no concurrent
    // writer.
    if unsafe { (*page).alloc_count } == 0 {
        std::process::abort();
    }
    // SAFETY: `block` is a user pointer the `# Safety` contract guarantees was
    // returned by a prior allocation in `page`/`segment`; non-nullness is the
    // allocator invariant, so `new_unchecked` is sound. Equality with
    // `page.free` is the double-free guard (the head was just freed).
    if Some(unsafe { NonNull::new_unchecked(block) }) == unsafe { (*page).free } {
        std::process::abort();
    }
    let was_full = unsafe { (*page).list_state } == 2;
    // SAFETY: `segment` is the live segment header owning `page` per the
    // `# Safety` contract and `page_index` is this page's index, satisfying
    // `cookie_for`'s contract.
    let encrypted = unsafe { Segment::free_list_encrypted(segment) };
    let cookie = unsafe { Segment::cookie_for_dynamic(segment, encrypted, page_index) };

    // Backward-edge canary check (HardenedPolicy with ENABLE_FREE_LIST_ENCRYPTION).
    // Under StandardPolicy this is dead code (ENABLE_FREE_LIST_ENCRYPTION = false).
    if P::ENABLE_FREE_LIST_ENCRYPTION {
        // SAFETY: `block` is a live, MIN_BLOCK_SIZE-aligned block per the
        // caller's contract; the canary slot lies at block+size_of::<Block>()
        // which is within the allocation by the MIN_BLOCK_SIZE constraint.
        if unsafe { mnemosyne_core::types::Block::check_double_free(block, cookie) } {
            std::process::abort();
        }
        // Write the canary so the next free of this block is detectable.
        // SAFETY: same slot bounds as the read above.
        unsafe { mnemosyne_core::types::Block::write_free_canary(block, cookie) };
    }

    // SAFETY: `block` points to a valid block in `page` per the `# Safety`
    // contract; writing its embedded next pointer reinitializes the free-list
    // link and stays inside the block this caller now owns.
    unsafe {
        (*block).set_next_dynamic((*page).free, encrypted, cookie);
    }
    // SAFETY: `block` is non-null (allocator invariant, re-confirmed by the
    // double-free guard above); publishing it as the new free-list head.
    unsafe { (*page).free = Some(NonNull::new_unchecked(block)) };

    // SAFETY: `segment`/`page`/`page_index` are the matching segment, page, and
    // its index per the `# Safety` contract; the decrement updates this page's
    // and segment's occupancy bookkeeping under the caller's exclusive access.
    let becomes_empty = unsafe {
        let count = (*page).alloc_count - 1;
        (*page).alloc_count = count;
        if count == 0 && !Segment::is_current(segment) {
            (*segment).page_occupied_mask &= !(1 << page_index);
        }
        count == 0
    };

    let class = unsafe { (*page).size_class } as usize;
    let page_ptr = unsafe { NonNull::new_unchecked(page) };

    with_page_list_token::<B, _>(|mut token| {
        // SAFETY: `page_ptr` is non-null (built above from the contract-valid `page`)
        // and belongs to the page lists the token brands.
        let branded_page = unsafe { token.page(page_ptr) };
        // SAFETY: the transitions below take the token that proves exclusive access to
        // this allocator's page lists, and `page`'s `list_state` names the list it
        // currently sits in, so unlink/move operate on a linked node.
        if was_full {
            if becomes_empty && !alloc.is_current_segment(segment) {
                // Case 1: Went from full directly to empty
                unsafe {
                    unlink_page_from_list(
                        &mut token,
                        alloc.full_pages.get_unchecked_mut(class),
                        branded_page,
                    );
                    push_page_front(&mut token, &mut alloc.empty_pages, branded_page, 3);
                }
            } else {
                // Case 2: Went from full to active.
                //
                // DELAY_PAGE_WAKE hysteresis (snmalloc 0.7.x `random_larger_thresholds`
                // / `waking` field): when the policy requests it, keep the page in the
                // full list until at least `capacity / WAKE_DENOMINATOR` blocks have
                // been freed. This prevents rapid LIFO address reuse and makes
                // use-after-free and heap-spray exploits harder to land by widening
                // the temporal window between free and realloc.
                //
                // Under `StandardPolicy`, `DELAY_PAGE_WAKE = false` and the compiler
                // eliminates the entire guard as dead code (zero-cost monomorphization).
                let max_blocks = mnemosyne_core::size_class::class_to_max_blocks(class);
                // SAFETY: `page` is exclusively owned per this function's contract;
                // `alloc_count` is a valid initialized field.
                let freed_so_far = max_blocks.saturating_sub(unsafe { (*page).alloc_count });
                let wake_threshold = max_blocks / (P::WAKE_DENOMINATOR as usize).max(1);
                if !P::DELAY_PAGE_WAKE || freed_so_far >= wake_threshold {
                    // SAFETY: `class < NUM_SIZE_CLASSES` — validated upstream;
                    // `branded_page` is exclusively owned by this thread and the
                    // page list operations preserve ownership invariants.
                    unsafe {
                        move_page_between_lists_branded(
                            &mut token,
                            alloc.full_pages.get_unchecked_mut(class),
                            alloc.active_pages.get_unchecked_mut(class),
                            branded_page,
                            1,
                        );
                    }
                }
                // When DELAY_PAGE_WAKE and threshold not yet reached, the page
                // stays in the full list; future frees will re-evaluate.
            }
        } else if becomes_empty && !alloc.is_current_segment(segment) {
            // Case 3: Went from active to empty (only if not the only active page)
            // SAFETY: `active_pages[class]` is this thread's own active-list head
            // and `page` is its live, owner-exclusive page, so the predicate's
            // head read is valid.
            let is_only_active =
                unsafe { is_sole_active_page(*alloc.active_pages.get_unchecked(class), page) };
            if !is_only_active {
                unsafe {
                    unlink_page_from_list(
                        &mut token,
                        alloc.active_pages.get_unchecked_mut(class),
                        branded_page,
                    );
                    push_page_front(&mut token, &mut alloc.empty_pages, branded_page, 3);
                }
            }
        }
    });

    becomes_empty
}
