use crate::local_alloc::page::{push_page_front, unlink_page_from_list, with_page_list_token};
use crate::local_alloc::ThreadAllocator;
use core::ptr::NonNull;
use mnemosyne_arena::{deallocate_segment, HasSegmentPool};
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Page, Segment, SegmentOwner};

const MIN_RETAINED_OWNED_SEGMENTS: usize = 3;
const RECLAIM_THRESHOLD_SEGMENTS: usize = MIN_RETAINED_OWNED_SEGMENTS + 1;

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Reclaims every segment owned by this thread cache back to the global
    /// pools, then clears the owned-segment chain so the operation is
    /// idempotent.
    pub fn reclaim_owned_segments(&mut self) {
        // We must clear the current segment first before deallocating any segments,
        // to avoid a use-after-free if the current segment gets deallocated.
        // SAFETY: `set_current_segment(None)` only clears this allocator's own
        // `current_segment`/`is_current` state; no segment pointer is read.
        unsafe { self.set_current_segment(None) };

        let mut curr = self.owned_segments_head;
        while !curr.is_null() {
            // SAFETY: `curr` walks this thread's own intrusive owned-segments
            // chain (`owned_segments_head` then each `next_owned_segment`), so
            // every node is a live segment owned exclusively by this allocator.
            // `next` is captured before `curr` is deallocated or pushed to the
            // orphan pool, so the walk never dereferences a freed segment. All
            // `pages[i]` reads use `i` drawn from `page_occupied_mask` bits,
            // which index valid entries of the segment's page array.
            unsafe {
                let next = (*curr).next_owned_segment;

                let dynamic_encrypted = (*curr).free_list_encrypted;
                let mut total_allocations = 0;
                let mut mask = (*curr).page_occupied_mask;
                while mask != 0 {
                    let i = mask.trailing_zeros() as usize;
                    mask &= mask - 1;
                    if i == 0 {
                        continue;
                    }
                    let page = &mut (*curr).pages[i];
                    let reclaimed =
                        page.reclaim_thread_free_if_present_for_segment(dynamic_encrypted, curr, i);
                    if reclaimed > 0 {
                        self.record_cross_thread_reclaimed(reclaimed);
                    }
                    total_allocations += page.alloc_count;
                }

                (*curr).owner = SegmentOwner::NONE;
                (*curr).owner_allocator = core::ptr::null_mut();
                (*curr).is_current = false;
                (*curr).next_owned_segment = core::ptr::null_mut();
                (*curr).prev_owned_segment = core::ptr::null_mut();

                if total_allocations == 0 {
                    deallocate_segment::<B>(curr);
                } else {
                    B::global_orphan_pool().push_unbounded(curr);
                }

                curr = next;
            }
        }
        self.next_page_index = 0;
        self.owned_segments_head = core::ptr::null_mut();
        self.owned_segment_count = 0;
        self.active_pages = [None; NUM_SIZE_CLASSES];
        self.full_pages = [None; NUM_SIZE_CLASSES];
        self.empty_pages = None;
    }

    /// Tries to reclaim a segment if it has zero active allocations.
    ///
    /// # Safety
    ///
    /// Accesses and modifies page and segment lists.
    pub unsafe fn try_reclaim_segment(&mut self, segment: *mut Segment) -> bool {
        if self
            .current_segment
            .is_some_and(|current| current.as_ptr() == segment)
        {
            return false;
        }

        if self.owned_segment_count < RECLAIM_THRESHOLD_SEGMENTS {
            return false;
        }

        // SAFETY: `segment` is a live segment owned by this allocator (the
        // caller's precondition; it is not `current_segment`, checked above).
        // Each `i` comes from a set bit of `page_occupied_mask`, so it indexes a
        // valid, occupied entry of the segment's page array, and `&mut pages[i]`
        // is unaliased because the segment is exclusive to this thread.
        unsafe {
            let dynamic_encrypted = (*segment).free_list_encrypted;
            let mut mask = (*segment).page_occupied_mask;
            while mask != 0 {
                let i = mask.trailing_zeros() as usize;
                mask &= mask - 1;
                if i == 0 {
                    continue;
                }
                let pg = &mut (*segment).pages[i];
                if pg.alloc_count > 0 {
                    let reclaimed = pg.reclaim_thread_free_if_present_for_segment(
                        dynamic_encrypted,
                        segment,
                        i,
                    );
                    if reclaimed > 0 {
                        self.record_cross_thread_reclaimed(reclaimed);
                    }
                    if pg.alloc_count > 0 {
                        return false;
                    }
                }
            }
        }

        // SAFETY: every occupied page of `segment` was just confirmed to have
        // zero live allocations, so detaching them from this allocator's page
        // lists and unlinking the segment from the owned chain leaves no live
        // references; both helpers operate only on this thread's own structures.
        unsafe {
            unlink_segment_pages(self, segment);
            self.unlink_owned_segment(segment);
        }

        // SAFETY: `segment` is now fully detached (no page-list or owned-list
        // membership; `unlink_owned_segment` already cleared both link fields),
        // so clearing its owner identity and returning it hands a segment with no
        // live references back to the pool exactly once.
        unsafe { detach_and_release_segment::<B>(segment) };
        true
    }

    /// Performs a defragmentation sweep over all owned segments (excluding `current_segment`),
    /// consolidating cross-thread frees, identifying empty pages, and reclaiming empty segments.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the allocator is in a safe, non-reentrant state.
    pub unsafe fn periodic_defragmentation_sweep<P: AllocPolicy>(&mut self) {
        let mut curr = self.owned_segments_head;
        while !curr.is_null() {
            let segment = curr;
            // SAFETY: `segment` is a live node of this thread's own
            // owned-segments chain; reading its `next_owned_segment` before any
            // teardown advances the walk without dereferencing a freed segment.
            curr = unsafe { (*segment).next_owned_segment };

            if self.is_current_segment(segment) {
                continue;
            }

            // SAFETY: `segment` is a live segment owned exclusively by this
            // thread, so reading its `free_list_encrypted` flag and
            // `page_occupied_mask` is a valid, unaliased load.
            let dynamic_encrypted = unsafe { (*segment).free_list_encrypted };
            let mut total_allocations = 0;

            if unsafe { (*segment).page_occupied_mask != 0 } {
                // SAFETY: `segment` is owned by this thread; each `i` is a set
                // bit of `page_occupied_mask`, indexing a valid occupied page,
                // so `&mut pages[i]` is in-bounds and unaliased. The page-list
                // token scopes the active/full/empty list mutations to this
                // allocator's own lists, and every `NonNull::new_unchecked`
                // wraps a non-null interior page pointer.
                unsafe {
                    with_page_list_token::<B, _>(|mut token| {
                        let mut mask = (*segment).page_occupied_mask;
                        while mask != 0 {
                            let i = mask.trailing_zeros() as usize;
                            mask &= mask - 1;
                            if i == 0 {
                                continue;
                            }
                            let pg = &mut (*segment).pages[i];
                            let reclaimed = pg.reclaim_thread_free_if_present_for_segment(
                                dynamic_encrypted,
                                segment,
                                i,
                            );
                            if reclaimed > 0 {
                                self.record_cross_thread_reclaimed(reclaimed);
                            }
                            total_allocations += pg.alloc_count;

                            if pg.alloc_count == 0 && (pg.list_state == 1 || pg.list_state == 2) {
                                let class = pg.size_class as usize;
                                // SAFETY: `active_pages[class]` is this thread's
                                // own active-list head and `pg` is a live,
                                // owner-exclusive page of this segment, so the
                                // predicate's head read is valid.
                                let is_only_active =
                                    crate::free::is_sole_active_page(self.active_pages[class], pg);
                                if !is_only_active {
                                    let pg_ptr = NonNull::new_unchecked(pg as *mut Page);
                                    let branded_page = token.page(pg_ptr);
                                    if pg.list_state == 1 {
                                        unlink_page_from_list(
                                            &mut token,
                                            self.active_pages.get_unchecked_mut(class),
                                            branded_page,
                                        );
                                    } else {
                                        unlink_page_from_list(
                                            &mut token,
                                            self.full_pages.get_unchecked_mut(class),
                                            branded_page,
                                        );
                                    }
                                    push_page_front(
                                        &mut token,
                                        &mut self.empty_pages,
                                        branded_page,
                                        3,
                                    );
                                }
                            }
                        }
                    });
                }
            }

            if total_allocations == 0 && self.owned_segment_count >= RECLAIM_THRESHOLD_SEGMENTS {
                // SAFETY: the sweep above observed zero live allocations across
                // every occupied page of `segment`, so detaching its pages and
                // unlinking it from the owned chain leaves no live references;
                // `detach_and_release_segment` then clears the owner identity and
                // returns the fully detached segment to the pool exactly once.
                // The next node was captured into `curr` before this teardown.
                unsafe {
                    unlink_segment_pages(self, segment);
                    self.unlink_owned_segment(segment);
                    detach_and_release_segment::<B>(segment);
                }
            }
        }
    }
}

/// Detaches every page of `segment` from `alloc`'s active/full/empty page lists.
///
/// # Safety
///
/// `segment` must be a live segment owned exclusively by `alloc`, and its
/// `page_linked_mask` must accurately mark the pages currently linked into
/// `alloc`'s page lists. The caller must hold exclusive access to `alloc`.
unsafe fn unlink_segment_pages<B: HasSegmentPool>(
    alloc: &mut ThreadAllocator<B>,
    segment: *mut Segment,
) {
    // SAFETY: `segment` is a live segment owned by `alloc` (caller contract), so
    // reading its `page_linked_mask` is a valid, unaliased load.
    let mut mask = unsafe { (*segment).page_linked_mask };
    while mask != 0 {
        let i = mask.trailing_zeros() as usize;
        mask &= mask - 1;
        // SAFETY: `i` is a set bit of `page_linked_mask`, indexing a valid page
        // of `segment`; the segment is exclusive to `alloc`, so `&mut pages[i]`
        // is unaliased.
        let pg = unsafe { &mut (*segment).pages[i] };
        let state = pg.list_state;
        if state == 1 || state == 2 {
            let class = pg.size_class as usize;
            // SAFETY: `list_state` 1/2 means `pg` is linked into the active/full
            // list for `class`; unlinking it from that list is the matching
            // operation on `alloc`'s own structures.
            unsafe { alloc.unlink_page(pg as *mut Page, class) };
        } else if state == 3 {
            // SAFETY: `list_state == 3` means `pg` is linked into `alloc`'s
            // empty-page list, the list this unlink operates on.
            unsafe { alloc.unlink_empty_page(pg as *mut Page) };
        }
    }
}

/// Clears a fully-detached segment's owner identity and returns it to the pool.
///
/// This is the shared teardown tail for `try_reclaim_segment` and
/// `periodic_defragmentation_sweep`: both call it only after
/// `unlink_owned_segment` has already spliced `segment` out of the owned chain
/// (clearing *both* `prev_owned_segment` and `next_owned_segment`) and
/// `unlink_segment_pages` has detached its pages, so the links need no
/// re-clearing here — the previous per-site code redundantly re-nulled
/// `next_owned_segment` while leaving `prev_owned_segment` untouched, an
/// asymmetry this consolidation removes.
///
/// # Safety
///
/// `segment` must be a live segment already unlinked from every owned-segment
/// and page list, so that clearing its owner identity and handing it to
/// `deallocate_segment` returns a segment with no live references exactly once.
#[inline]
unsafe fn detach_and_release_segment<B: HasSegmentPool>(segment: *mut Segment) {
    // SAFETY: `segment` is the fully-detached live segment per the contract;
    // writing its owner identity and releasing it is a valid, exclusive final
    // access before ownership returns to the pool.
    unsafe {
        (*segment).owner = SegmentOwner::NONE;
        (*segment).owner_allocator = core::ptr::null_mut();
        deallocate_segment::<B>(segment);
    }
}
