//! Page allocation-count and segment-occupancy bookkeeping.
//!
//! These `impl Page` methods maintain `alloc_count` and the parent segment's
//! `page_occupied_mask`; they are split from the page type definition by
//! Separation of Concerns (occupancy accounting vs. layout/allocation).

use crate::abort::abort_on_corruption;
use crate::types::{Page, Segment};

impl Page {
    /// Sets `alloc_count` addressing the page through its containing segment,
    /// updating the segment's hierarchical `page_occupied_mask` on a
    /// zero/non-zero transition.
    ///
    /// # Why these are segment-addressed rather than `&mut self`
    ///
    /// Every function here writes page metadata *and* reaches the parent
    /// segment header. With a `&mut self` receiver those two accesses descend
    /// from different pointers — the page borrow covers only the page's bytes —
    /// so the segment access invalidates the borrow. Miri reports it as UB (see
    /// the `reclaim` module's note). Projecting the page out of `segment` puts
    /// both on one provenance. The page argument is redundant anyway: segment
    /// plus index determines it.
    ///
    /// # Safety
    ///
    /// `segment` must be a valid segment header and `page_index` must be in
    /// range of its `pages` array.
    #[inline(always)]
    pub unsafe fn set_alloc_count_in_segment(
        segment: *mut Segment,
        page_index: usize,
        count: usize,
    ) {
        debug_assert!(page_index < crate::constants::PAGES_PER_SEGMENT);
        // SAFETY: valid header and in-range index per this function's contract,
        // so the projection stays inside the segment and inherits its
        // provenance.
        let page = unsafe { &raw mut (*segment).pages[page_index] };
        // SAFETY: `page` addresses initialized page metadata inside `segment`.
        let old = unsafe { (*page).alloc_count };
        if old == count {
            return;
        }
        // SAFETY: as above.
        unsafe { (*page).alloc_count = count };
        // SAFETY: the caller's `# Safety` contract guarantees `segment` is this
        // page's parent segment header, so dereferencing it to read
        // `is_current` is a valid read of initialized segment metadata.
        if (old == 0) != (count == 0) && (count > 0 || unsafe { !(*segment).is_current }) {
            // SAFETY: same precondition — `segment` is the valid parent segment
            // and `page_index` is in range (`debug_assert`ed above), so the
            // occupancy-bit update targets a valid `page_occupied_mask`.
            unsafe { Self::set_segment_page_occupied(segment, page_index, count > 0) };
        }
    }

    /// Increments `alloc_count`, setting the segment occupancy bit only on the
    /// empty-to-occupied transition.
    ///
    /// # Safety
    ///
    /// Carries [`Page::set_alloc_count_in_segment`]'s contract.
    #[inline(always)]
    pub unsafe fn increment_alloc_count_in_segment(segment: *mut Segment, page_index: usize) {
        debug_assert!(page_index < crate::constants::PAGES_PER_SEGMENT);
        // SAFETY: valid header and in-range index, as above.
        let page = unsafe { &raw mut (*segment).pages[page_index] };
        // SAFETY: `page` addresses initialized page metadata inside `segment`.
        let old = unsafe { (*page).alloc_count };
        // SAFETY: as above.
        unsafe { (*page).alloc_count = old + 1 };
        if old == 0 {
            // SAFETY: valid parent header and in-range index, so the
            // occupancy-bit update is valid.
            unsafe { Self::set_segment_page_occupied(segment, page_index, true) };
        }
    }

    /// Decrements `alloc_count`, clearing the segment occupancy bit only on the
    /// occupied-to-empty transition.
    ///
    /// # Safety
    ///
    /// Carries [`Page::set_alloc_count_in_segment`]'s contract.
    #[inline(always)]
    pub unsafe fn decrement_alloc_count_in_segment(segment: *mut Segment, page_index: usize) {
        debug_assert!(page_index < crate::constants::PAGES_PER_SEGMENT);
        // SAFETY: valid header and in-range index, as above.
        let page = unsafe { &raw mut (*segment).pages[page_index] };
        // SAFETY: `page` addresses initialized page metadata inside `segment`.
        if unsafe { (*page).alloc_count } == 0 {
            abort_on_corruption("decrement_alloc_count on a page with zero live allocations");
        }
        // SAFETY: as above; the zero case aborted.
        let count = unsafe { (*page).alloc_count } - 1;
        // SAFETY: as above.
        unsafe { (*page).alloc_count = count };
        // SAFETY: valid parent header, so reading `is_current` is valid.
        if count == 0 && unsafe { !(*segment).is_current } {
            // SAFETY: same header and in-range index, so clearing the occupancy
            // bit is valid.
            unsafe { Self::set_segment_page_occupied(segment, page_index, false) };
        }
    }

    #[inline(always)]
    unsafe fn set_segment_page_occupied(segment: *mut Segment, page_index: usize, occupied: bool) {
        let mask = 1 << page_index;
        // SAFETY: every caller establishes that `segment` is a valid,
        // initialized parent segment header and `page_index < PAGES_PER_SEGMENT`
        // (so `mask` stays within the 32-bit `page_occupied_mask`). The write is
        // performed by the page's owner under the segment-ownership protocol, so
        // no concurrent writer races this non-atomic field.
        unsafe {
            if occupied {
                (*segment).page_occupied_mask |= mask;
            } else {
                (*segment).page_occupied_mask &= !mask;
            }
        }
    }
}
