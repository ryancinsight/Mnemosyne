//! Page free lists: reclamation, the wake threshold, and the seeded
//! permutation that interleaves the primary and secondary heads.

use super::*;
use crate::types::{Block, Page, Segment};
use ::std::alloc::{alloc_zeroed, dealloc};
use core::ptr::NonNull;

#[test]
fn test_page_reclaim_thread_free() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };
    // The page is addressed through `segment_ptr` for the whole test rather
    // than through a long-lived `&mut Page`. Pushing to `thread_free` and
    // reclaiming both reach the segment header (for the free-list cookie), and
    // a page borrow held across those calls sits on a different provenance than
    // the segment access, which invalidates it. In production those pushes come
    // from a *remote* thread that holds no page borrow at all, so addressing by
    // segment is also the faithful shape.
    const PAGE_INDEX: usize = 1;
    let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
    unsafe { (*page).block_size = 16 };

    unsafe {
        let page_start = Page::page_start_in_segment(segment_ptr, PAGE_INDEX);
        Page::initialize_free_list_in_segment::<crate::policy::StandardPolicy>(
            segment_ptr,
            PAGE_INDEX,
            page_start,
            0,
        );
    }

    let first = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };
    unsafe { (*page).alloc_count = 1 };
    unsafe {
        (*page)
            .thread_free
            .push::<crate::policy::StandardPolicy>(first)
    };

    let reclaimed =
        unsafe { Page::reclaim_thread_free_if_present_in_segment(segment_ptr, PAGE_INDEX, false) };

    assert_eq!(reclaimed, 1);
    assert_eq!(unsafe { (*page).alloc_count }, 0);
    assert_eq!(unsafe { (*page).free }, Some(first));
    assert!(
        unsafe { (*page).thread_free.is_empty() },
        "thread_free list was not empty after reclaim"
    );

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}
#[test]
fn test_page_reclaim_thread_free_hot_path() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };
    // Addressed through `segment_ptr` throughout: pushing to `thread_free` and
    // reclaiming both read the segment header for the free-list cookie, and a
    // `&mut Page` held across those accesses is invalidated by them — Tree
    // Borrows disables the page tag, Stacked Borrows pops it.
    const PAGE_INDEX: usize = 1;
    let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
    unsafe { (*page).block_size = 16 };

    unsafe {
        let page_start = Page::page_start_in_segment(segment_ptr, PAGE_INDEX);
        Page::initialize_free_list_in_segment::<crate::policy::StandardPolicy>(
            segment_ptr,
            PAGE_INDEX,
            page_start,
            0,
        );
    }

    let b1 = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };
    let b2 = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };

    // Simulate all other blocks allocated / empty free list
    unsafe {
        (*page).free = None;
        (*page).alloc_count = 2;
        (*page)
            .thread_free
            .push::<crate::policy::StandardPolicy>(b1);
        (*page)
            .thread_free
            .push::<crate::policy::StandardPolicy>(b2);
    }

    // Reclaim thread_free. Since page.free is None, this triggers O(1) swap.
    let reclaimed = unsafe {
        Page::reclaim_thread_free_for_policy::<crate::policy::StandardPolicy>(
            segment_ptr,
            PAGE_INDEX,
        )
    };

    assert_eq!(reclaimed, 2);
    assert_eq!(unsafe { (*page).alloc_count }, 0);
    assert_eq!(unsafe { (*page).free }, Some(b2));

    unsafe {
        let next_node = (*b2.as_ptr()).get_next::<crate::policy::StandardPolicy>(0);
        assert_eq!(next_node, Some(b1));
        assert_eq!(
            (*b1.as_ptr()).get_next::<crate::policy::StandardPolicy>(0),
            None
        );
    }
    assert!(
        unsafe { (*page).thread_free.is_empty() },
        "thread_free list was not empty after reclaim"
    );

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}
#[test]
fn page_wake_hysteresis_uses_single_threshold_source() {
    let class = 0;
    let denom = 4;
    let max_blocks = crate::size_class::class_to_max_blocks(class);
    let threshold = Page::wake_threshold_for_class(class, denom);

    assert_eq!(threshold, max_blocks / denom.max(1));
    assert!(!Page::should_reactivate_after_free(
        class,
        max_blocks - 1,
        denom
    ));
    assert!(Page::should_reactivate_after_free(
        class,
        max_blocks - threshold,
        denom
    ));
}
#[test]
fn randomized_page_free_list_uses_seeded_permutation() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };
    // Addressed through the segment: free-list initialization reads the
    // segment's cookie, and a page borrow held across that access is
    // invalidated by it.
    const PAGE_INDEX: usize = 1;
    let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
    unsafe {
        (*page).block_size = 16;
        (*page).size_class = 0;
    }

    unsafe {
        let page_start = Page::page_start_in_segment(segment_ptr, PAGE_INDEX);
        Page::initialize_free_list_in_segment::<RandomizedTestPolicy>(
            segment_ptr,
            PAGE_INDEX,
            page_start,
            (7 << 16) | 5,
        );

        // Both heads must point into this page, which is what "kept a head"
        // means -- `is_some` would also accept a wild pointer from a
        // mis-seeded permutation, and that is the defect the interleaved
        // build can produce.
        let primary = (*page).free.expect("randomized page keeps a primary head");
        let secondary = (*page)
            .secondary_free
            .expect("randomized page keeps a secondary head");
        let page_end = page_start.addr() + crate::constants::PAGE_SIZE;
        for (head, which) in [(primary, "primary"), (secondary, "secondary")] {
            let addr = head.as_ptr().addr();
            assert!(
                (page_start.addr()..page_end).contains(&addr),
                "the {which} head must point into this page, not to {addr:#x}"
            );
        }
        assert_ne!(
            primary, secondary,
            "the two lists must not share a head block"
        );
        let (primary, secondary) = (Some(primary), Some(secondary));

        let expected_head = if Page::prefer_secondary_free(page, (*page).alloc_count as usize) {
            secondary
        } else {
            primary
        };
        let first = Page::pop_block::<RandomizedTestPolicy>(page);
        assert_eq!(
            Some(first),
            expected_head,
            "the seeded random policy must choose the active free-list head"
        );

        let cookie = Segment::cookie_for::<RandomizedTestPolicy>(segment_ptr, PAGE_INDEX);
        let second = Page::pop_block::<RandomizedTestPolicy>(page);
        let expected_second = (*first.as_ptr()).get_next::<RandomizedTestPolicy>(cookie);
        assert_eq!(
            Some(second),
            expected_second,
            "the second pop must follow the active list's next link"
        );

        dealloc(segment_ptr as *mut u8, layout);
    }
}
#[test]
fn reclaim_if_present_for_policy_keeps_randomized_head_selection() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };

    const PAGE_INDEX: usize = 1;
    let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
    let page_start = unsafe { Page::page_start_in_segment(segment_ptr, PAGE_INDEX) };
    unsafe {
        (*page).block_size = 16;
        (*page).size_class = 0;
        (*page).alloc_count = 1;
        (*page).free = None;
        (*page).secondary_free = None;
    }

    let block_a = unsafe { NonNull::new_unchecked(page_start.add(0) as *mut Block) };
    let block_b = unsafe { NonNull::new_unchecked(page_start.add(16) as *mut Block) };
    let remote = unsafe { NonNull::new_unchecked(page_start.add(32) as *mut Block) };
    unsafe {
        (*block_a.as_ptr()).set_next::<RandomizedTestPolicy>(None, 0);
        (*block_b.as_ptr()).set_next::<RandomizedTestPolicy>(None, 0);
        (*page).secondary_free = Some(block_a);
        (*page).free = Some(block_b);
        (*page).thread_free.push::<RandomizedTestPolicy>(remote);
    }

    let reclaimed = unsafe {
        Page::reclaim_thread_free_if_present_for_policy::<RandomizedTestPolicy>(
            segment_ptr,
            PAGE_INDEX,
        )
    };
    assert_eq!(
        reclaimed, 1,
        "the remote-free drain must reclaim the queued block"
    );

    let (expected_head, _expected_secondary) =
        unsafe { Page::choose_free_head(page, (*page).alloc_count as usize, true) };
    let active_head = unsafe {
        if (*page).secondary_free.is_some() && (*page).free.is_some() {
            if Page::prefer_secondary_free(page, (*page).alloc_count as usize) {
                (*page).secondary_free
            } else {
                (*page).free
            }
        } else {
            (*page).secondary_free.or((*page).free)
        }
    };
    assert_eq!(
        active_head, expected_head,
        "randomized remote-free drain must preserve the active free-list head"
    );

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}
#[test]
fn standard_policy_keeps_secondary_free_list_active_when_present() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };

    const PAGE_INDEX: usize = 1;
    let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
    let page_start = unsafe { Page::page_start_in_segment(segment_ptr, PAGE_INDEX) };
    unsafe {
        (*page).block_size = 16;
        (*page).size_class = 0;
        (*page).free = None;
        (*page).secondary_free = None;
    }

    let block_a = unsafe { NonNull::new_unchecked(page_start.add(0) as *mut Block) };
    let block_b = unsafe { NonNull::new_unchecked(page_start.add(16) as *mut Block) };
    unsafe {
        (*block_a.as_ptr()).set_next::<crate::policy::StandardPolicy>(Some(block_b), 0);
        (*block_b.as_ptr()).set_next::<crate::policy::StandardPolicy>(None, 0);
        (*page).secondary_free = Some(block_a);
    }

    let first = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };
    assert_eq!(
        Some(first),
        Some(block_a),
        "standard policy must pop the active secondary list head"
    );

    let second = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };
    assert_eq!(
        Some(second),
        Some(block_b),
        "standard policy must preserve the secondary list order"
    );

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}
