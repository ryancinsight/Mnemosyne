//! Per-backend orphan-pool draining logic.
//!
//! When a thread allocator is destroyed before all its segments are freed (e.g.
//! a crash or early exit), the live segments land in the global orphan pool.
//! This module drains those pools: it reclaims cross-thread frees back into
//! each page's local list, and deallocates any segment that becomes fully
//! empty. Segments that still carry live allocations are re-enqueued in the
//! orphan pool for the next decay step.

use core::sync::atomic::Ordering;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::types::{Page, Segment, SegmentOwner};

/// Drains the orphan pool for backend `B`.
///
/// Pops every segment, reclaims cross-thread frees into each page's local
/// list, and either returns the segment to the OS (if empty) or re-enqueues
/// it in the orphan pool (if live allocations remain).
pub(super) fn drain_orphan_pool<B: HasSegmentPool>() {
    let pool = B::global_orphan_pool();
    let mut retained_head = core::ptr::null_mut::<Segment>();

    while let Some(segment) = pool.pop() {
        // SAFETY: segment was exclusively obtained by popping from the pool.
        let dynamic_encrypted = unsafe { (*segment).free_list_encrypted };
        let mut total_allocations = 0;

        // SAFETY: `segment` is exclusively owned; `page_occupied_mask` is a
        // valid initialized header field.
        let mut mask = unsafe { (*segment).page_occupied_mask };
        while mask != 0 {
            let i = mask.trailing_zeros() as usize;
            mask &= mask - 1;
            if i == 0 {
                continue;
            }
            // Address pages through the segment pointer, not a `&mut Page`,
            // because reclaim reads the segment header for the free-list cookie
            // and a page borrow held across that access carries a different
            // provenance root.
            // SAFETY: `i < PAGES_PER_SEGMENT` (from the occupied mask) and
            // `segment` is exclusively owned at this point.
            let page = unsafe { &raw mut (*segment).pages[i] };
            // SAFETY: `segment` is exclusively owned and `i` is in-range.
            unsafe {
                let randomized = (*page).secondary_free.is_some();
                Page::reclaim_thread_free_if_present_in_segment_with_randomized(
                    segment,
                    i,
                    dynamic_encrypted,
                    randomized,
                );
            }
            // SAFETY: `page` is inside the exclusively-owned segment's pages array.
            total_allocations += unsafe { (*page).alloc_count };
        }

        if total_allocations == 0 {
            // All pages are empty — return the segment mapping to the OS.
            // SAFETY: `segment` is exclusively owned and all its pages are empty.
            unsafe {
                Segment::set_owner_allocator(segment, core::ptr::null_mut());
                Segment::set_owner(segment, SegmentOwner::NONE);
                (*segment).next_owned_segment = core::ptr::null_mut();
                (*segment).prev_owned_segment = core::ptr::null_mut();
                mnemosyne_arena::deallocate_segment::<B>(segment);
            }
        } else {
            // Live allocations remain — link into the local retain chain.
            // SAFETY: `segment` is exclusively owned; `store` is the standard
            // way to link it into a local chain before re-enqueuing.
            unsafe {
                (*segment)
                    .next_free_segment
                    .store(retained_head, Ordering::Relaxed);
            }
            retained_head = segment;
        }
    }

    // Re-enqueue segments that still had live allocations.
    let mut curr = retained_head;
    while !curr.is_null() {
        // SAFETY: `curr` is a node of the locally-built retained chain;
        // loading its `next_free_segment` before re-caching is sound.
        let next = unsafe { (*curr).next_free_segment.load(Ordering::Relaxed) };
        // SAFETY: `curr` is exclusively owned; clearing the link and pushing
        // back to the orphan pool transfers ownership to the pool.
        unsafe {
            (*curr)
                .next_free_segment
                .store(core::ptr::null_mut(), Ordering::Relaxed);
            pool.push_unbounded(curr);
        }
        curr = next;
    }
}
