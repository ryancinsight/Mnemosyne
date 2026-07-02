//! Page allocation-count and segment-occupancy bookkeeping.
//!
//! These `impl Page` methods maintain `alloc_count` and the parent segment's
//! `page_occupied_mask`; they are split from the page type definition by
//! Separation of Concerns (occupancy accounting vs. layout/allocation).

use crate::abort::abort_on_corruption;
use crate::types::{Page, Segment};

impl Page {
    /// Sets the active allocation count for this page and updates the parent
    /// segment's hierarchical `page_occupied_mask` bit vector in-place.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the parent segment is a valid Segment mapping.
    #[inline(always)]
    pub unsafe fn set_alloc_count(&mut self, count: usize) {
        let old = self.alloc_count;
        if old == count {
            return;
        }
        if (old == 0) != (count == 0) {
            let segment = self.parent_segment();
            let idx = self.page_index as usize;
            // SAFETY: `parent_segment` returns this page's parent segment header
            // (every `Page` lives inside its segment's `pages` array), and `idx`
            // is this page's own `page_index`, so the per-segment precondition of
            // `set_alloc_count_for_segment` holds.
            unsafe { self.set_alloc_count_for_segment(segment, idx, count) };
        } else {
            self.alloc_count = count;
        }
    }

    /// Sets `alloc_count` when the caller already knows the containing segment
    /// and page index.
    ///
    /// # Safety
    ///
    /// `segment` must be this page's parent segment, and `page_index` must be
    /// this page's index in `segment.pages`.
    #[inline(always)]
    pub unsafe fn set_alloc_count_for_segment(
        &mut self,
        segment: *mut Segment,
        page_index: usize,
        count: usize,
    ) {
        debug_assert!(page_index < crate::constants::PAGES_PER_SEGMENT);
        let old = self.alloc_count;
        if old == count {
            return;
        }
        self.alloc_count = count;
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

    /// Increments `alloc_count`, updating the segment occupancy bit only on
    /// the empty-to-occupied transition.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the parent segment is a valid Segment mapping.
    #[inline(always)]
    pub unsafe fn increment_alloc_count(&mut self) {
        let old = self.alloc_count;
        self.alloc_count = old + 1;
        if old == 0 {
            let segment = self.parent_segment();
            let idx = self.page_index as usize;
            // SAFETY: `parent_segment` returns `self`'s parent header and `idx`
            // is this page's index, satisfying the per-segment precondition of
            // `set_segment_page_occupied`.
            unsafe { Self::set_segment_page_occupied(segment, idx, true) };
        }
    }

    /// Increments `alloc_count` when the caller already knows the containing
    /// segment and page index.
    ///
    /// # Safety
    ///
    /// `segment` must be this page's parent segment, and `page_index` must be
    /// this page's index in `segment.pages`.
    #[inline(always)]
    pub unsafe fn increment_alloc_count_for_segment(
        &mut self,
        segment: *mut Segment,
        page_index: usize,
    ) {
        debug_assert!(page_index < crate::constants::PAGES_PER_SEGMENT);
        let old = self.alloc_count;
        self.alloc_count = old + 1;
        if old == 0 {
            // SAFETY: caller's `# Safety` contract guarantees `segment` is the
            // parent header and `page_index` is in range (`debug_assert`ed
            // above), so the occupancy-bit update is valid.
            unsafe { Self::set_segment_page_occupied(segment, page_index, true) };
        }
    }

    /// Decrements `alloc_count`, updating the segment occupancy bit only on
    /// the occupied-to-empty transition.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the parent segment is a valid Segment mapping.
    #[inline(always)]
    pub unsafe fn decrement_alloc_count(&mut self) {
        if self.alloc_count == 0 {
            abort_on_corruption("decrement_alloc_count on a page with zero live allocations");
        }
        let count = self.alloc_count - 1;
        self.alloc_count = count;
        if count == 0 {
            let segment = self.parent_segment();
            let idx = self.page_index as usize;
            // SAFETY: `parent_segment` returns `self`'s parent header, so reading
            // `is_current` is a valid read of initialized metadata.
            if unsafe { !(*segment).is_current } {
                // SAFETY: same parent-segment header and in-range `idx`, so the
                // occupancy-bit clear targets a valid `page_occupied_mask`.
                unsafe { Self::set_segment_page_occupied(segment, idx, false) };
            }
        }
    }

    /// Decrements `alloc_count` when the caller already knows the containing
    /// segment and page index.
    ///
    /// # Safety
    ///
    /// `segment` must be this page's parent segment, and `page_index` must be
    /// this page's index in `segment.pages`.
    #[inline(always)]
    pub unsafe fn decrement_alloc_count_for_segment(
        &mut self,
        segment: *mut Segment,
        page_index: usize,
    ) {
        debug_assert!(page_index < crate::constants::PAGES_PER_SEGMENT);
        if self.alloc_count == 0 {
            abort_on_corruption(
                "decrement_alloc_count_for_segment on a page with zero live allocations",
            );
        }
        let count = self.alloc_count - 1;
        self.alloc_count = count;
        // SAFETY: caller's `# Safety` contract guarantees `segment` is the
        // parent header, so reading `is_current` is valid.
        if count == 0 && unsafe { !(*segment).is_current } {
            // SAFETY: same parent header and in-range `page_index`
            // (`debug_assert`ed above), so clearing the occupancy bit is valid.
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
