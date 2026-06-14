use crate::local_alloc::ThreadAllocator;
use mnemosyne_arena::{deallocate_segment, HasSegmentPool};
use mnemosyne_core::constants::PAGES_PER_SEGMENT;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Page, Segment, SegmentOwner};

const MIN_RETAINED_OWNED_SEGMENTS: usize = 3;
const RECLAIM_THRESHOLD_SEGMENTS: usize = MIN_RETAINED_OWNED_SEGMENTS + 1;

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Reclaims every segment owned by this thread cache back to the global
    /// pools, then clears the owned-segment chain so the operation is
    /// idempotent.
    pub fn reclaim_owned_segments(&mut self) {
        let mut curr = self.owned_segments_head;
        while !curr.is_null() {
            unsafe {
                let next = (*curr).next_owned_segment;

                let dynamic_encrypted = (*curr).free_list_encrypted;
                let mut total_allocations = 0;
                for i in 1..PAGES_PER_SEGMENT {
                    let page = &mut (*curr).pages[i];
                    let reclaimed =
                        page.reclaim_thread_free_dynamic_for_segment(dynamic_encrypted, curr, i);
                    if reclaimed > 0 {
                        crate::local_alloc::record_cross_thread_reclaimed(reclaimed);
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
        self.owned_segments_head = core::ptr::null_mut();
        self.owned_segment_count = 0;
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
                if pg.thread_free.is_empty() {
                    return false;
                }
                let reclaimed =
                    pg.reclaim_thread_free_dynamic_for_segment(dynamic_encrypted, segment, i);
                if reclaimed > 0 {
                    crate::local_alloc::record_cross_thread_reclaimed(reclaimed);
                }
                if pg.alloc_count > 0 {
                    return false;
                }
            }
        }

        unsafe {
            unlink_segment_pages(self, segment);
            self.unlink_owned_segment(segment);
        }

        if self.current_segment.is_some_and(|p| p.as_ptr() == segment) {
            unsafe { self.set_current_segment(None) };
            self.next_page_index = 0;
        }

        unsafe {
            (*segment).owner = SegmentOwner::NONE;
            (*segment).owner_allocator = core::ptr::null_mut();
            (*segment).next_owned_segment = core::ptr::null_mut();
            deallocate_segment::<B>(segment);
        }
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
            curr = unsafe { (*segment).next_owned_segment };

            if self.is_current_segment(segment) {
                continue;
            }

            let dynamic_encrypted = unsafe { (*segment).free_list_encrypted };
            let mut total_allocations = 0;

            if unsafe { (*segment).page_occupied_mask != 0 } {
                let mut mask = unsafe { (*segment).page_occupied_mask };
                while mask != 0 {
                    let i = mask.trailing_zeros() as usize;
                    mask &= mask - 1;
                    if i == 0 {
                        continue;
                    }
                    let pg = unsafe { &mut (*segment).pages[i] };
                    if !pg.thread_free.is_empty() {
                        let reclaimed = unsafe {
                            pg.reclaim_thread_free_dynamic_for_segment(
                                dynamic_encrypted,
                                segment,
                                i,
                            )
                        };
                        if reclaimed > 0 {
                            crate::local_alloc::record_cross_thread_reclaimed(reclaimed);
                        }
                    }
                    total_allocations += pg.alloc_count;

                    if pg.alloc_count == 0 && (pg.list_state == 1 || pg.list_state == 2) {
                        let class = pg.size_class as usize;
                        let is_only_active = self.active_pages[class].is_some_and(|head| {
                            core::ptr::eq(head.as_ptr(), pg)
                                && unsafe { (*head.as_ptr()).next_page.is_none() }
                        });
                        if !is_only_active {
                            unsafe {
                                self.unlink_page(pg as *mut Page, class);
                                self.push_empty_page(core::ptr::NonNull::new_unchecked(
                                    pg as *mut Page,
                                ));
                            }
                        }
                    }
                }
            }

            if total_allocations == 0 && self.owned_segment_count >= RECLAIM_THRESHOLD_SEGMENTS {
                unsafe {
                    unlink_segment_pages(self, segment);
                    self.unlink_owned_segment(segment);
                    (*segment).owner = SegmentOwner::NONE;
                    (*segment).owner_allocator = core::ptr::null_mut();
                    (*segment).next_owned_segment = core::ptr::null_mut();
                    deallocate_segment::<B>(segment);
                }
            }
        }
    }
}

unsafe fn unlink_segment_pages<B: HasSegmentPool>(
    alloc: &mut ThreadAllocator<B>,
    segment: *mut Segment,
) {
    for i in 1..PAGES_PER_SEGMENT {
        let pg = unsafe { &mut (*segment).pages[i] };
        if pg.block_size > 0 {
            let class = pg.size_class as usize;
            unsafe { alloc.unlink_page(pg as *mut Page, class) };
        }
        unsafe { alloc.unlink_empty_page(pg as *mut Page) };
    }
}
