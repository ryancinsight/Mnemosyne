//! Page cross-thread free-list reclamation.
//!
//! These functions atomically drain a page's `thread_free` (cross-thread
//! deallocation) queue back into the page-local free list, validating the
//! drained chain; split from the page type definition by Separation of
//! Concerns.
//!
//! # Why these take a segment and index rather than `&mut Page`
//!
//! A `Page` lives inside its parent `Segment`'s `pages` array, and reclamation
//! needs both: the page's queue and the segment's cookie and occupancy mask.
//! Taking `&mut self` made those two accesses descend from *different*
//! pointers — the page borrow covers only the page's bytes, while the segment
//! pointer covers the whole mapping — so touching the segment invalidated the
//! page borrow. Miri reported it as UB two ways: a wildcard segment read that
//! would remove the strongly-protected `&mut Page` argument, and a failed
//! two-phase retag of a page borrow the segment access had already popped.
//!
//! Deriving the page pointer *from* the segment pointer instead gives every
//! access in this module one shared provenance, so no borrow can invalidate
//! another. That is also what makes the page argument redundant: the segment
//! and index determine it, and accepting it separately is what let the two
//! provenances diverge in the first place.

use crate::abort::abort_on_corruption;
use crate::types::{Page, Segment};

impl Page {
    /// Returns a pointer to `page_index`'s page, derived from `segment` so it
    /// shares the segment mapping's provenance.
    ///
    /// # Safety
    ///
    /// `segment` must be a valid segment header and `page_index` must be in
    /// range of its `pages` array.
    #[inline(always)]
    unsafe fn page_in_segment(segment: *mut Segment, page_index: usize) -> *mut Page {
        debug_assert!(page_index < crate::constants::PAGES_PER_SEGMENT);
        // SAFETY: the caller guarantees `segment` is a valid header and
        // `page_index` is in range, so projecting to that element stays inside
        // the segment allocation and inherits its provenance.
        unsafe { &raw mut (*segment).pages[page_index] }
    }

    /// Atomically drains cross-thread frees into the page-local free list.
    ///
    /// Returns the number of blocks reclaimed.
    ///
    /// # Safety
    ///
    /// `segment` must be a valid parent segment header, `page_index` must be
    /// this page's in-range index in `segment.pages`, and the page must belong
    /// to the allocator context currently reconciling its metadata.
    #[inline]
    pub unsafe fn reclaim_thread_free_in_segment(
        segment: *mut Segment,
        page_index: usize,
        encrypted: bool,
    ) -> usize {
        // SAFETY: forwarded from this function's contract — valid header, index
        // in range.
        let page = unsafe { Self::page_in_segment(segment, page_index) };

        // SAFETY: `segment` is the valid parent header and `page_index` is in
        // range, satisfying `cookie_for_dynamic`'s contract. This read and every
        // page access below descend from the same `segment` pointer, so neither
        // invalidates the other.
        let cookie = unsafe { Segment::cookie_for_dynamic(segment, encrypted, page_index) };

        // SAFETY: `page` points at initialized page metadata inside `segment`.
        let Some((block, count)) = (unsafe { (*page).thread_free.pop_all(encrypted, cookie) })
        else {
            return 0;
        };

        // SAFETY: as above.
        let alloc_count = unsafe { (*page).alloc_count };
        if count > alloc_count {
            abort_on_corruption(
                "reclaimed cross-thread free count exceeds the page's live allocations",
            );
        }
        // SAFETY: `segment`/`page_index` are the caller-provided valid parent
        // header and in-range index; `count <= alloc_count` was just checked, so
        // the subtraction does not underflow.
        unsafe { Self::set_alloc_count_in_segment(segment, page_index, alloc_count - count) };

        // SAFETY: as above.
        let block_size = unsafe { (*page).block_size };
        let page_start = (segment as usize) + (page_index << crate::constants::PAGE_SHIFT);
        let page_end = page_start + crate::constants::PAGE_SIZE;

        let mut last = block;
        let first_addr = last.as_ptr() as usize;
        if first_addr < page_start
            || first_addr + block_size > page_end
            || (first_addr & (crate::constants::MIN_BLOCK_SIZE - 1)) != 0
        {
            abort_on_corruption(
                "reclaimed cross-thread free chain head is outside its page or misaligned",
            );
        }

        let mut visited = 1;
        // SAFETY: `last` starts at the validated `block` head and each loop
        // iteration only advances to a `node` that is re-validated below to lie
        // within the page and be `MIN_BLOCK_SIZE`-aligned, so every
        // `last.as_ptr()` deref reads a valid, aligned `Block` taken from this
        // page's thread-free chain.
        while let Some(node) = unsafe { (*last.as_ptr()).get_next_dynamic(encrypted, cookie) } {
            visited += 1;
            if visited > count {
                abort_on_corruption(
                    "reclaimed cross-thread free chain is longer than its counted length",
                );
            }
            let node_addr = node.as_ptr() as usize;
            if node_addr < page_start
                || node_addr + block_size > page_end
                || (node_addr & (crate::constants::MIN_BLOCK_SIZE - 1)) != 0
            {
                abort_on_corruption(
                    "reclaimed cross-thread free node is outside its page or misaligned",
                );
            }
            last = node;
        }
        if visited != count {
            abort_on_corruption(
                "reclaimed cross-thread free chain is shorter than its counted length",
            );
        }

        // SAFETY: `page` is valid initialized page metadata; `last` is the
        // validated tail node of the drained chain (in bounds of the page and
        // aligned), so splicing the existing free list onto it is a valid,
        // owner-exclusive write of a `Block` this thread now owns.
        unsafe {
            let existing = (*page).free;
            if existing.is_none() {
                (*page).free = Some(block);
            } else {
                (*last.as_ptr()).set_next_dynamic(existing, encrypted, cookie);
                (*page).free = Some(block);
            }
        }
        count
    }

    /// Drains cross-thread frees only when the page-local queue is currently
    /// non-empty.
    ///
    /// This keeps sweep-style callers from issuing an atomic `pop_all` for
    /// pages that have no remote frees while preserving the same reclamation
    /// logic when the queue is populated.
    ///
    /// # Safety
    ///
    /// Carries [`Page::reclaim_thread_free_in_segment`]'s contract unchanged.
    #[inline]
    pub unsafe fn reclaim_thread_free_if_present_in_segment(
        segment: *mut Segment,
        page_index: usize,
        encrypted: bool,
    ) -> usize {
        // SAFETY: forwarded from this function's contract.
        let page = unsafe { Self::page_in_segment(segment, page_index) };
        // SAFETY: `page` points at initialized page metadata inside `segment`.
        if unsafe { (*page).thread_free.is_empty() } {
            return 0;
        }
        // SAFETY: preconditions forwarded unchanged.
        unsafe { Self::reclaim_thread_free_in_segment(segment, page_index, encrypted) }
    }

    /// Policy-typed wrapper over [`Page::reclaim_thread_free_in_segment`].
    ///
    /// # Safety
    ///
    /// Carries [`Page::reclaim_thread_free_in_segment`]'s contract unchanged.
    #[inline]
    pub unsafe fn reclaim_thread_free_for_policy<P: crate::policy::AllocPolicy>(
        segment: *mut Segment,
        page_index: usize,
    ) -> usize {
        // SAFETY: preconditions forwarded unchanged.
        unsafe {
            Self::reclaim_thread_free_in_segment(
                segment,
                page_index,
                P::ENABLE_FREE_LIST_ENCRYPTION,
            )
        }
    }
}
