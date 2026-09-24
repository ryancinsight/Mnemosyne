//! Free helpers the sibling crates call with an allocator already in hand.

use crate::ThreadAllocator;
use crate::free_helpers::is_sole_active_page;
use crate::local_alloc::page::{move_page_raw, push_page_front_raw, unlink_page_from_list_raw};
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

    // SAFETY: `alloc: &mut ThreadAllocator<B>` proves exclusive access to the
    // page lists; each branch below only touches the list that `list_state`
    // names, which the caller guarantees contains `page_ptr`.
    if was_full {
        if becomes_empty && !alloc.is_current_segment(segment) {
            // Case 1: Full → empty
            unsafe {
                unlink_page_from_list_raw(page_ptr, alloc.full_pages.get_unchecked_mut(class));
                push_page_front_raw(page_ptr, &mut alloc.empty_pages, 3);
            }
        } else {
            // Case 2: Full → active (with optional DELAY_PAGE_WAKE hysteresis).
            let max_blocks = mnemosyne_core::size_class::class_to_max_blocks(class);
            // SAFETY: `page` is exclusively owned per this function's contract.
            let freed_so_far =
                max_blocks.saturating_sub(unsafe { (*page).alloc_count } as usize);
            let wake_threshold = max_blocks / (P::WAKE_DENOMINATOR as usize).max(1);
            if !P::DELAY_PAGE_WAKE || freed_so_far >= wake_threshold {
                unsafe {
                    move_page_raw(
                        page_ptr,
                        alloc.full_pages.get_unchecked_mut(class),
                        alloc.active_pages.get_unchecked_mut(class),
                        1,
                    );
                }
            }
            // When DELAY_PAGE_WAKE and threshold not yet reached, the page
            // stays in the full list; future frees will re-evaluate.
        }
    } else if becomes_empty && !alloc.is_current_segment(segment) {
        // Case 3: Active → empty (only when not the sole active page)
        // SAFETY: `active_pages[class]` is this thread's own active-list head.
        let is_only_active =
            unsafe { is_sole_active_page(*alloc.active_pages.get_unchecked(class), page) };
        if !is_only_active {
            unsafe {
                unlink_page_from_list_raw(page_ptr, alloc.active_pages.get_unchecked_mut(class));
                push_page_front_raw(page_ptr, &mut alloc.empty_pages, 3);
            }
        }
    }

    becomes_empty
}
