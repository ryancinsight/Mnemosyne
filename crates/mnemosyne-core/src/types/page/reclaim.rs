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
    pub unsafe fn page_in_segment(segment: *mut Segment, page_index: usize) -> *mut Page {
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
        randomized: bool,
    ) -> usize {
        // SAFETY: forwarded from this function's contract — valid header, index
        // in range.
        let page = unsafe { Self::page_in_segment(segment, page_index) };

        if !unsafe { Segment::free_list_mode_matches(segment, encrypted) } {
            abort_on_corruption(
                "free-list mode mismatch: reclaim raw path does not match the segment",
            );
        }

        // Hot path: when the segment is not using encrypted free-list links, the
        // cookie is irrelevant and can be kept at zero to avoid the parent-segment
        // walk on the standard allocator path. The encrypted branch preserves the
        // original cookie contract exactly.
        let cookie = if encrypted {
            // SAFETY: `segment` is the valid parent header and `page_index` is in
            // range, satisfying `cookie_for_dynamic`'s contract. This read and every
            // page access below descend from the same `segment` pointer, so neither
            // invalidates the other.
            unsafe { Segment::cookie_for_dynamic(segment, encrypted, page_index) }
        } else {
            0
        };

        // SAFETY: `page` points at initialized page metadata inside `segment`.
        // The raw hot path remains valid only while the parent segment is still in
        // the unencrypted mode; if the metadata changed without going through the
        // dynamic path, this would bypass the XOR-encrypted node encoding.
        let reclaimed = if encrypted {
            unsafe { (*page).thread_free.pop_all(encrypted, cookie) }
        } else {
            unsafe { (*page).thread_free.pop_all_raw() }
        };
        let Some((block, count)) = reclaimed else {
            return 0;
        };

        // SAFETY: as above.
        let alloc_count = unsafe { (*page).alloc_count } as usize;
        if count > alloc_count {
            abort_on_corruption(
                "reclaimed cross-thread free count exceeds the page's live allocations",
            );
        }
        // SAFETY: `segment`/`page_index` are the caller-provided valid parent
        // header and in-range index; `count <= alloc_count` was just checked, so
        // the subtraction does not underflow. Reuse the already-provenanced page
        // pointer and the just-read `alloc_count` to avoid re-deriving the page
        // and re-reading the same metadata while we update the occupancy bit.
        let new_count = alloc_count - count;
        let new_count_u32 = u32::try_from(new_count).unwrap_or_else(|_| {
            abort_on_corruption("reclaimed cross-thread free drained past u32 alloc_count range");
        });
        unsafe {
            Self::update_alloc_count_in_segment(
                segment,
                page,
                page_index,
                alloc_count as u32,
                new_count_u32,
            )
        };

        // SAFETY: as above.
        let block_size = unsafe { (*page).block_size } as usize;
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
        while let Some(node) = if encrypted {
            unsafe { (*last.as_ptr()).get_next_dynamic(encrypted, cookie) }
        } else {
            // SAFETY: the segment mode check above committed this page to the raw
            // free-list path, so the next pointer is the unencoded representation and
            // the cookie does not need to be consulted for traversal.
            unsafe { (*last.as_ptr()).get_next_raw() }
        } {
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
            let randomized = randomized || (*page).secondary_free.is_some();
            let (existing, use_secondary) = Page::choose_free_head(page, new_count, randomized);
            if existing.is_none() {
                if use_secondary {
                    (*page).secondary_free = Some(block);
                } else {
                    (*page).free = Some(block);
                }
            } else {
                (*last.as_ptr()).set_next_dynamic(existing, encrypted, cookie);
                if use_secondary {
                    (*page).secondary_free = Some(block);
                } else {
                    (*page).free = Some(block);
                }
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
        let randomized = unsafe { (*page).secondary_free.is_some() };
        unsafe {
            Self::reclaim_thread_free_if_present_in_segment_with_randomized(
                segment, page_index, encrypted, randomized,
            )
        }
    }

    /// Drains a page's remote-free queue when present while preserving the
    /// caller's random-preserve choice for the page-local free list.
    ///
    /// This is the idle/decay fast path: the queue only needs a single empty
    /// check, but the re-merge into the page-local free list must still keep the
    /// page's randomized ordering if the policy or the active secondary list
    /// requires it.
    ///
    /// # Safety
    /// `segment` must point at a live segment and `page_index` must be one of
    /// its pages. `encrypted` must equal the segment's recorded free-list mode
    /// -- a mismatch aborts rather than encoding a link the owner cannot
    /// decode (ADR 0001). `randomized` only selects an ordering and cannot
    /// make a valid link invalid.
    #[inline]
    pub unsafe fn reclaim_thread_free_if_present_in_segment_with_randomized(
        segment: *mut Segment,
        page_index: usize,
        encrypted: bool,
        randomized: bool,
    ) -> usize {
        // SAFETY: forwarded from this function's contract.
        let page = unsafe { Self::page_in_segment(segment, page_index) };
        if !unsafe { Segment::free_list_mode_matches(segment, encrypted) } {
            abort_on_corruption(
                "free-list mode mismatch: reclaim raw path does not match the segment",
            );
        }
        // SAFETY: `page` points at initialized page metadata inside `segment`.
        if unsafe { (*page).thread_free.is_empty() } {
            return 0;
        }
        // SAFETY: preconditions forwarded unchanged.
        unsafe { Self::reclaim_thread_free_in_segment(segment, page_index, encrypted, randomized) }
    }

    /// Policy-typed wrapper over the present-only remote-free drain.
    ///
    /// # Safety
    /// `segment` must point at a live segment and `page_index` must be one of
    /// its pages. `P` selects the TLS slot and the allocator's compile-time
    /// semantics only: the encoding mode is read from the segment itself, so a
    /// policy whose `ENABLE_FREE_LIST_ENCRYPTION` differs from the segment's
    /// recorded mode is sound here and aborts nothing.
    #[inline]
    pub unsafe fn reclaim_thread_free_if_present_for_policy<P: crate::policy::AllocPolicy>(
        segment: *mut Segment,
        page_index: usize,
    ) -> usize {
        // SAFETY: forwarded from the caller's contract — the segment and page
        // remain live, and only the page's recorded mode decides how its
        // remote-free chain is encoded. The caller's policy remains relevant for
        // TLS-slot selection and the allocator's compile-time semantics, but it
        // must never override the owning segment's runtime free-list mode.
        let page = unsafe { Self::page_in_segment(segment, page_index) };
        let encrypted = unsafe { Segment::free_list_encrypted(segment) };
        let randomized = unsafe { (*page).secondary_free.is_some() };
        unsafe {
            Self::reclaim_thread_free_if_present_in_segment_with_randomized(
                segment, page_index, encrypted, randomized,
            )
        }
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
        // SAFETY: preconditions forwarded unchanged. The segment's recorded
        // `free_list_encrypted` bit is the authoritative source of truth for the
        // queue encoding; the caller policy affects only the TLS slot and the
        // compile-time semantics of the allocator instance, not the live page's
        // link format.
        let page = unsafe { Self::page_in_segment(segment, page_index) };
        let encrypted = unsafe { Segment::free_list_encrypted(segment) };
        let randomized = unsafe { (*page).secondary_free.is_some() };
        unsafe { Self::reclaim_thread_free_in_segment(segment, page_index, encrypted, randomized) }
    }
}
