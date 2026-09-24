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
/// Delegates to the non-generic [`do_local_free_internal_raw`] by passing
/// the three P:: constants as plain booleans. This means `StandardPolicy`
/// and `SecurePolicy` — which share `(enable_encryption=false, delay_wake=false,
/// wake_denominator=4)` — compile the entire body exactly once per `B`.
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
    // SAFETY: forwarded unchanged.
    unsafe {
        do_local_free_internal_raw::<B>(
            alloc,
            block,
            page,
            segment,
            page_index,
            P::ENABLE_FREE_LIST_ENCRYPTION,
            P::DELAY_PAGE_WAKE,
            P::WAKE_DENOMINATOR,
        )
    }
}

/// Non-generic SSOT for the local free path.
///
/// All P:: constants are passed as plain booleans so the body compiles once per
/// `(enable_encryption, delay_wake, wake_denominator, B)` combination rather
/// than once per `(P, B)`. StandardPolicy and SecurePolicy share
/// `(false, false, 4, B)`, cutting the instantiation count from 3×N to 2×N.
///
/// # Safety
///
/// Same contract as `do_local_free_internal_policy`.
#[inline(always)]
unsafe fn do_local_free_internal_raw<B: HasSegmentPool>(
    alloc: &mut ThreadAllocator<B>,
    block: *mut Block,
    page: *mut Page,
    segment: *mut Segment,
    page_index: usize,
    enable_encryption: bool,
    delay_wake: bool,
    wake_denominator: u16,
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
    // `# Safety` contract and `page_index` is this page's index.
    let encrypted = unsafe { Segment::free_list_encrypted(segment) };
    let cookie = unsafe { Segment::cookie_for_dynamic(segment, encrypted, page_index) };

    // Backward-edge canary check: only when the policy enables free-list
    // encryption (HardenedPolicy). Dead code for StandardPolicy/SecurePolicy.
    if enable_encryption {
        // SAFETY: `block` is a live, MIN_BLOCK_SIZE-aligned block per the
        // caller's contract; the canary slot is within the allocation.
        if unsafe { mnemosyne_core::types::Block::check_double_free(block, cookie) } {
            std::process::abort();
        }
        // SAFETY: same slot bounds as the read above.
        unsafe { mnemosyne_core::types::Block::write_free_canary(block, cookie) };
    }

    // SAFETY: `block` points to a valid block in `page`; writing its embedded
    // next pointer reinitializes the free-list link.
    unsafe {
        (*block).set_next_dynamic((*page).free, encrypted, cookie);
    }
    // SAFETY: `block` is non-null (allocator invariant confirmed above).
    unsafe { (*page).free = Some(NonNull::new_unchecked(block)) };

    // SAFETY: same triple contract as above; the decrement updates occupancy.
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

    // SAFETY: `alloc: &mut ThreadAllocator<B>` proves exclusive access.
    if was_full {
        if becomes_empty && !alloc.is_current_segment(segment) {
            // Case 1: Full → empty
            unsafe {
                unlink_page_from_list_raw(page_ptr, alloc.full_pages.get_unchecked_mut(class));
                push_page_front_raw(page_ptr, &mut alloc.empty_pages, 3);
            }
        } else {
            // Case 2: Full → active (with optional DELAY_PAGE_WAKE hysteresis).
            // Uses Page::should_reactivate_after_free as the SSOT for the
            // freed_so_far >= wake_threshold condition.
            // SAFETY: `page` is exclusively owned per this function's contract.
            let should_wake = !delay_wake
                || unsafe {
                    Page::should_reactivate_after_free(
                        class,
                        (*page).alloc_count as usize,
                        wake_denominator as usize,
                    )
                };
            if should_wake {
                unsafe {
                    move_page_raw(
                        page_ptr,
                        alloc.full_pages.get_unchecked_mut(class),
                        alloc.active_pages.get_unchecked_mut(class),
                        1,
                    );
                }
            }
            // When delay_wake and threshold not yet reached, the page stays
            // in the full list; future frees will re-evaluate.
        }
    } else if becomes_empty && !alloc.is_current_segment(segment) {
        // Case 3: Active → empty (only when not the sole active page)
        // SAFETY: `active_pages[class]` is this thread's own active-list head;
        // `page` is live and exclusively owned by this allocator.
        let is_only_active =
            unsafe { is_sole_active_page(*alloc.active_pages.get_unchecked(class), page) };
        if !is_only_active {
            // SAFETY: `&mut alloc` proves exclusive access; `page_ptr` is linked
            // in `active_pages[class]` (list_state == 1 for non-full pages).
            unsafe {
                unlink_page_from_list_raw(page_ptr, alloc.active_pages.get_unchecked_mut(class));
                push_page_front_raw(page_ptr, &mut alloc.empty_pages, 3);
            }
        }
    }

    becomes_empty
}
