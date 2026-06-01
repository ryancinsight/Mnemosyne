use crate::local_alloc::ThreadAllocator;
use mnemosyne_arena::{deallocate_segment, HasSegmentPool};
use mnemosyne_core::constants::{PAGES_PER_SEGMENT, PAGE_SIZE};
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Page, Segment, SegmentOwner};

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Prepends `segment` to this thread's intrusive doubly-linked
    /// owned-segments list and stamps the ownership token.
    ///
    /// This is the single authoritative insertion point for the owned-segments
    /// list; both the fresh-segment and orphan-adoption paths route through it
    /// so the `prev`/`next` invariant is maintained in exactly one place.
    ///
    /// # Safety
    ///
    /// `segment` must be a live segment owned exclusively by this allocator and
    /// must not already be linked into any owned-segments list.
    #[inline]
    pub(crate) unsafe fn push_owned_segment<P: AllocPolicy>(&mut self, segment: *mut Segment) {
        // Safety: `segment` is exclusive to this allocator; the caller
        // guarantees it is unlinked, so overwriting its link fields and
        // relinking the current head is sound.
        unsafe {
            #[cfg(all(windows, target_arch = "x86_64"))]
            {
                let tid = {
                    let val: u32;
                    core::arch::asm!(
                        "mov {0:e}, gs:[0x48]",
                        out(reg) val,
                        options(nostack, preserves_flags, readonly)
                    );
                    val
                };
                (*segment).owner = SegmentOwner::from_thread_id(tid);
            }
            #[cfg(not(all(windows, target_arch = "x86_64")))]
            {
                (*segment).owner = SegmentOwner::from_ptr(self as *mut ThreadAllocator<B>);
            }
            (*segment).prev_owned_segment = core::ptr::null_mut();
            (*segment).next_owned_segment = self.owned_segments_head;
            if !self.owned_segments_head.is_null() {
                (*self.owned_segments_head).prev_owned_segment = segment;
            }
            self.owned_segments_head = segment;

            if P::ENABLE_FREE_LIST_ENCRYPTION {
                self.initialize_segment_keys(segment);
            }
        }
    }

    /// Populates the keys array of a newly acquired segment using the thread-local seed.
    ///
    /// # Safety
    ///
    /// `segment` must point to a valid, writable `Segment`.
    #[inline]
    pub unsafe fn initialize_segment_keys(&mut self, segment: *mut Segment) {
        let seed = super::get_tls_seed();
        let segment_addr = segment as usize;
        unsafe {
            (*segment).free_list_encrypted = true;
            for i in 0..PAGES_PER_SEGMENT {
                (*segment).keys[i] = (segment_addr.wrapping_add(i * PAGE_SIZE)) ^ seed;
            }
        }
    }

    /// Unlinks a segment from the owned segments list in O(1).
    ///
    /// The list is intrusive and doubly linked, so the segment's own
    /// `prev_owned_segment`/`next_owned_segment` pointers locate both
    /// neighbours directly; no linear search for the predecessor is required.
    /// Both link fields are cleared so the detached segment carries no stale
    /// pointers into the list.
    #[inline]
    pub(crate) unsafe fn unlink_owned_segment(&mut self, segment: *mut Segment) {
        // Safety: `segment` is a node owned by this allocator's list; its
        // neighbour pointers are maintained by `push_owned_segment` and this
        // method, so splicing through them mutates only live list nodes.
        unsafe {
            let prev = (*segment).prev_owned_segment;
            let next = (*segment).next_owned_segment;
            if prev.is_null() {
                // `segment` was the head.
                self.owned_segments_head = next;
            } else {
                (*prev).next_owned_segment = next;
            }
            if !next.is_null() {
                (*next).prev_owned_segment = prev;
            }
            (*segment).prev_owned_segment = core::ptr::null_mut();
            (*segment).next_owned_segment = core::ptr::null_mut();
        }
    }

    /// Reclaims every segment owned by this thread cache back to the global
    /// pools, then clears the owned-segment chain so the operation is
    /// idempotent.
    pub fn reclaim_owned_segments(&mut self) {
        // When the thread exits, we must reclaim all owned segments.
        let mut curr = self.owned_segments_head;
        while !curr.is_null() {
            // Safety: curr is a valid pointer in the owned segments chain.
            // We traverse the pages inside it, pop all cross-thread frees, and either deallocate or orphan it.
            unsafe {
                let next = (*curr).next_owned_segment;

                let dynamic_encrypted = (*curr).free_list_encrypted;
                let mut total_allocations = 0;
                for i in 1..PAGES_PER_SEGMENT {
                    let page = &mut (*curr).pages[i];
                    let reclaimed = page.reclaim_thread_free_dynamic(dynamic_encrypted);
                    if reclaimed > 0 {
                        super::record_cross_thread_reclaimed(reclaimed);
                    }
                    total_allocations += page.alloc_count;
                }

                (*curr).owner = SegmentOwner::NONE;
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

        let mut count = 0;
        let mut curr = self.owned_segments_head;
        while !curr.is_null() {
            count += 1;
            if count >= 4 {
                break;
            }
            curr = unsafe { (*curr).next_owned_segment };
        }
        if count < 4 {
            return false;
        }

        // Safety: segment is a valid pointer to a segment owned by us.
        unsafe {
            let dynamic_encrypted = (*segment).free_list_encrypted;
            if (*segment).page_occupied_mask != 0 {
                for i in 1..PAGES_PER_SEGMENT {
                    let pg = &mut (*segment).pages[i];

                    if pg.alloc_count > 0 {
                        if pg.thread_free.is_empty() {
                            return false;
                        }
                        let reclaimed = pg.reclaim_thread_free_dynamic(dynamic_encrypted);
                        if reclaimed > 0 {
                            super::record_cross_thread_reclaimed(reclaimed);
                        }
                        if pg.alloc_count > 0 {
                            return false;
                        }
                    }
                }
            }
        }

        // Safety: segment is valid. We unlink all its pages from their size class lists.
        unsafe {
            for i in 1..PAGES_PER_SEGMENT {
                let pg = &mut (*segment).pages[i];
                if pg.block_size > 0 {
                    let class = pg.size_class as usize;
                    self.unlink_page(pg as *mut Page, class);
                }
                self.unlink_empty_page(pg as *mut Page);
            }

            self.unlink_owned_segment(segment);
        }

        if self.current_segment.is_some_and(|p| p.as_ptr() == segment) {
            self.set_current_segment(None);
            self.next_page_index = 0;
        }

        // Safety: segment is unlinked and exclusive to us. We clear fields and deallocate.
        unsafe {
            (*segment).owner = SegmentOwner::NONE;
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
            // Advance `curr` immediately, because we might unlink and deallocate `segment`.
            curr = unsafe { (*segment).next_owned_segment };

            // Skip the current active segment.
            if self.is_current_segment(segment) {
                continue;
            }

            let dynamic_encrypted = unsafe { (*segment).free_list_encrypted };
            let mut total_allocations = 0;

            if unsafe { (*segment).page_occupied_mask != 0 } {
                // 1. First pass: drain remote frees on all pages of this segment.
                for i in 1..PAGES_PER_SEGMENT {
                    let pg = unsafe { &mut (*segment).pages[i] };
                    if pg.block_size > 0 {
                        let reclaimed = pg.reclaim_thread_free_dynamic(dynamic_encrypted);
                        if reclaimed > 0 {
                            super::record_cross_thread_reclaimed(reclaimed);
                        }
                        total_allocations += pg.alloc_count;

                        // If page has zero active allocations and is currently linked in active/full lists,
                        // move it to the empty_pages recycling stack (if it's not the last active page of its class).
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
            }

            // 2. If the segment is completely empty (zero active allocations across all pages),
            // and we have more than 3 segments in our owned list, reclaim it.
            if total_allocations == 0 {
                let mut segment_count = 0;
                let mut scan = self.owned_segments_head;
                while !scan.is_null() {
                    segment_count += 1;
                    if segment_count >= 4 {
                        break;
                    }
                    scan = unsafe { (*scan).next_owned_segment };
                }

                if segment_count >= 4 {
                    // Unlink all pages of this segment from whichever list they are in
                    for i in 1..PAGES_PER_SEGMENT {
                        let pg = unsafe { &mut (*segment).pages[i] };
                        if pg.block_size > 0 {
                            let class = pg.size_class as usize;
                            unsafe {
                                self.unlink_page(pg as *mut Page, class);
                            }
                        }
                        unsafe {
                            self.unlink_empty_page(pg as *mut Page);
                        }
                    }

                    // Unlink segment and deallocate it back to the global pool
                    unsafe {
                        self.unlink_owned_segment(segment);
                        (*segment).owner = SegmentOwner::NONE;
                        (*segment).next_owned_segment = core::ptr::null_mut();
                        deallocate_segment::<B>(segment);
                    }
                }
            }
        }
    }
}
