use super::page::{pop_page_free_block, try_allocate_page_local, try_reclaim_and_allocate};
use crate::local_alloc::ThreadAllocator;
use core::ptr::NonNull;
use mnemosyne_arena::{allocate_segment, HasSegmentPool};
use mnemosyne_core::constants::PAGES_PER_SEGMENT;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::size_class::class_to_size;
#[cfg(test)]
use mnemosyne_core::size_class::size_to_class;
use mnemosyne_core::types::{Page, Segment};

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Allocates a small memory block of the specified size class.
    ///
    /// # Safety
    ///
    /// `class` must be a valid size class index (< `NUM_SIZE_CLASSES`).
    #[inline(always)]
    pub unsafe fn alloc_class<P: AllocPolicy>(&mut self, class: usize) -> *mut u8 {
        if let Some(mut page_ptr) = unsafe { *self.active_pages.get_unchecked(class) } {
            // Safety: page_ptr points to a valid Page structure inside an active segment owned by us.
            let page = unsafe { page_ptr.as_mut() };

            // 1. Check thread-local free list or lazy bump allocation.
            if let Some(block) = unsafe { try_allocate_page_local::<P>(page) } {
                return block.as_ptr() as *mut u8;
            }

            // 2. Reclaim batched cross-thread frees only after the local list is empty.
            // Safety: `page` is owned by this allocator and `try_reclaim_and_allocate`
            // upholds the `Page::reclaim_thread_free` contract on its behalf.
            if let Some(block) =
                unsafe { try_reclaim_and_allocate::<P>(page, &mut self.cross_thread_reclaimed) }
            {
                return block.as_ptr() as *mut u8;
            }
        }

        // Outline the cold allocation path to keep alloc() small and fast.
        // SAFETY: `class` is the same caller-validated size-class index
        // (< `NUM_SIZE_CLASSES`, the contract of `alloc_class`) that indexed
        // `active_pages` above, satisfying `alloc_cold`'s bounds precondition.
        unsafe { self.alloc_cold::<P>(class) }
    }

    /// Allocates a block of memory of the given size.
    ///
    /// Returns null if the size is not a small class or if allocation fails.
    ///
    /// This size-taking entry point exists only for the crate's own tests, which
    /// drive the allocator by byte size; production callers route through
    /// `alloc_class` (size-class already resolved) or the crate's public
    /// `thread_alloc*` entry points, so it is gated out of non-test builds to
    /// keep the public unsafe surface minimal.
    ///
    /// # Safety
    ///
    /// This method is unsafe because it works with raw pointers and handles manual memory layouts.
    #[cfg(test)]
    #[inline(always)]
    pub unsafe fn alloc<P: AllocPolicy>(&mut self, size: usize) -> *mut u8 {
        let class = match size_to_class(size) {
            Some(c) => c,
            None => return core::ptr::null_mut(),
        };
        unsafe { self.alloc_class::<P>(class) }
    }

    /// Cold path for allocating a block when active pages are full.
    ///
    /// Marked as `#[inline(never)]` to prevent pollution of instruction cache.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the allocator is in a valid state and the size class
    /// index is within bounds of the `active_pages` array.
    #[inline(never)]
    pub unsafe fn alloc_cold<P: AllocPolicy>(&mut self, class: usize) -> *mut u8 {
        unsafe { self.record_defrag_operation::<P>() };
        // 1. Move the current active page to full_pages if it is indeed full.
        if let Some(mut active_ptr) = unsafe { *self.active_pages.get_unchecked(class) } {
            let active_page = active_ptr.as_mut();
            // Safety: `active_page` is owned by this allocator.
            if let Some(block) =
                try_reclaim_and_allocate::<P>(active_page, &mut self.cross_thread_reclaimed)
            {
                return block.as_ptr() as *mut u8;
            }
            // The page is truly full! Move it to full_pages.
            unsafe {
                self.unlink_page(active_ptr.as_ptr(), class);
                self.push_full_page(active_ptr, class);
            }
        }

        // 1b. Check if the new head of active_pages can satisfy the allocation.
        if let Some(mut active_ptr) = unsafe { *self.active_pages.get_unchecked(class) } {
            let active_page = active_ptr.as_mut();
            if let Some(block) = unsafe { try_allocate_page_local::<P>(active_page) } {
                return block.as_ptr() as *mut u8;
            }
            if let Some(block) = unsafe {
                try_reclaim_and_allocate::<P>(active_page, &mut self.cross_thread_reclaimed)
            } {
                return block.as_ptr() as *mut u8;
            }
        }

        // 2. Check if any page in full_pages has local free blocks or reclaimed cross-thread frees!
        // Also limit loop to 128 pages to bound search latency under threaded saturation.
        let mut curr_opt = unsafe { *self.full_pages.get_unchecked(class) };
        let mut checked = 0;
        while let Some(mut page_ptr) = curr_opt {
            if checked >= 128 {
                break;
            }
            checked += 1;
            let page = page_ptr.as_mut();
            // Safety: `page` is owned by this allocator. Since it is in full_pages,
            // we know it has no local free blocks. We only need to check for cross-thread frees to reclaim.
            let block_opt =
                unsafe { try_reclaim_and_allocate::<P>(page, &mut self.cross_thread_reclaimed) };
            if let Some(block) = block_opt {
                if page.alloc_count < page.max_blocks() {
                    // Page is no longer full! Move it back to active list.
                    // SAFETY: `page_ptr` is a live `Page` owned by this
                    // allocator (just walked from `full_pages[class]`), and
                    // `class` is the caller-validated size-class index keying
                    // that list, so it matches the page's own size class.
                    unsafe {
                        let _ = self.move_full_page_to_active(page_ptr, class);
                    }
                }
                return block.as_ptr() as *mut u8;
            }
            curr_opt = page.next_page;
        }

        // 3. Allocate a brand new page
        // SAFETY: `class` is the caller-validated size-class index, satisfying
        // `get_new_page`'s implicit bounds expectation; it returns either null
        // (handled below) or a page freshly installed into `active_pages`.
        let new_page_ptr = self.get_new_page::<P>(class);
        if new_page_ptr.is_null() {
            return core::ptr::null_mut();
        }
        self.page_refills += 1;

        // SAFETY: `new_page_ptr` is the non-null pointer just returned by
        // `get_new_page`, pointing to a freshly initialized `Page` inside a
        // segment owned exclusively by this thread, so the `&mut` is unaliased.
        let page = &mut *new_page_ptr;
        // Safety: `get_new_page` guarantees a freshly initialized page whose
        // `initialize_free_list` populated `free` with at least one block.
        let block = pop_page_free_block::<P>(page);

        page.increment_alloc_count();

        // If it becomes full immediately, move to full list
        if page.alloc_count == page.max_blocks() {
            // SAFETY: `new_page_ptr` is the non-null page from `get_new_page`,
            // so `NonNull::new_unchecked` is valid; `class` is the
            // caller-validated size class the page was installed under, keeping
            // the unlink-then-push consistent with the page's list membership.
            unsafe {
                let ptr = NonNull::new_unchecked(new_page_ptr);
                self.unlink_page(ptr.as_ptr(), class);
                self.push_full_page(ptr, class);
            }
        }
        block.as_ptr() as *mut u8
    }

    /// Obtains a new page for the given size class.
    ///
    /// # Safety
    ///
    /// Accesses and modifies segment pointers.
    pub(crate) unsafe fn get_new_page<P: AllocPolicy>(&mut self, class: usize) -> *mut Page {
        let block_size = class_to_size(class);

        // Check if there is an empty page in the defragmentation list first.
        if let Some(mut page_ptr) = unsafe { self.pop_best_empty_page() } {
            unsafe {
                let random_value = if P::RANDOMIZE_ALLOCATION {
                    self.next_random() ^ page_ptr.as_ptr() as u64 ^ (class as u64).rotate_left(17)
                } else {
                    0
                };
                let page = page_ptr.as_mut();

                let page_start = page.page_start();
                page.block_size = block_size;
                page.size_class = class as u8;
                page.initialize_free_list::<P>(page_start, random_value);

                self.push_active_page(page_ptr, class);
                self.recycled_pages += 1;
                return page_ptr.as_ptr();
            }
        }

        // Prefer never-used pages in the current segment.
        if self.current_segment.is_none() || self.next_page_index >= PAGES_PER_SEGMENT {
            // Safety: acquires a policy-compatible segment from the OS/pools;
            // policy-incompatible orphans are returned to the orphan pool.
            if let Some(seg_ptr) = unsafe { acquire_policy_compatible_segment::<P, B>() } {
                // Determine if this is an orphaned segment vs a fresh/reinitialized segment.
                // An orphaned segment has pages[1].block_size > 0.
                // SAFETY: `seg_ptr` is the non-null segment just returned by
                // `allocate_segment`; reading `pages[1].block_size` from its
                // initialized mapping distinguishes a previously-used (orphan)
                // segment from a fresh one.
                let is_orphan = unsafe { (*seg_ptr).pages[1].block_size > 0 };

                if is_orphan {
                    self.orphan_segments_adopted += 1;
                    let mut found_page: *mut Page = core::ptr::null_mut();
                    // SAFETY: `seg_ptr` is a live, mapped segment from
                    // `allocate_segment` and is now claimed exclusively by this
                    // thread; `push_owned_segment` stamps ownership before any
                    // other thread can observe it. Every `pages[i]` for
                    // `i in 1..PAGES_PER_SEGMENT` is within the segment's page
                    // array, so each `&mut (*seg_ptr).pages[i]` is in-bounds and
                    // unaliased, and each `NonNull::new_unchecked(page_ptr)`
                    // wraps a non-null interior pointer into that array.
                    unsafe {
                        self.push_owned_segment::<P>(seg_ptr);

                        self.set_current_segment(Some(NonNull::new_unchecked(seg_ptr)));
                        self.next_page_index = PAGES_PER_SEGMENT;

                        (*seg_ptr).page_linked_mask = 0;

                        for i in 1..PAGES_PER_SEGMENT {
                            let page_ptr = &mut (*seg_ptr).pages[i] as *mut Page;
                            let page = &mut *page_ptr;

                            if page.block_size > 0 {
                                // Reclaim cross-thread frees to get accurate count.
                                // The orphan's chains are encoded under the
                                // segment's recorded mode; after the
                                // policy-compatibility gate in
                                // `acquire_policy_compatible_segment` it equals
                                // `P::ENABLE_FREE_LIST_ENCRYPTION`, but the
                                // dynamic flag is the authoritative source,
                                // matching every sweep-path reclaim.
                                let encrypted = (*seg_ptr).free_list_encrypted;
                                debug_assert_eq!(
                                    encrypted,
                                    P::ENABLE_FREE_LIST_ENCRYPTION,
                                    "adopted an orphan whose free-list mode does not match the policy"
                                );
                                let reclaimed = page.reclaim_thread_free_if_present_for_segment(
                                    encrypted, seg_ptr, i,
                                );
                                if reclaimed > 0 {
                                    self.record_cross_thread_reclaimed(reclaimed);
                                }

                                if page.alloc_count > 0 {
                                    let pg_class = page.size_class as usize;
                                    let ptr = NonNull::new_unchecked(page_ptr);
                                    if page.alloc_count < page.max_blocks() {
                                        self.push_active_page(ptr, pg_class);
                                    } else {
                                        self.push_full_page(ptr, pg_class);
                                    }
                                } else if found_page.is_null() {
                                    found_page = page_ptr;
                                } else {
                                    self.push_empty_page(NonNull::new_unchecked(page_ptr));
                                }
                            } else if found_page.is_null() {
                                found_page = page_ptr;
                            } else {
                                self.push_empty_page(NonNull::new_unchecked(page_ptr));
                            }
                        }
                    }

                    if !found_page.is_null() {
                        let random_value = if P::RANDOMIZE_ALLOCATION {
                            self.next_random() ^ found_page as u64 ^ (class as u64).rotate_left(17)
                        } else {
                            0
                        };
                        let page = unsafe { &mut *found_page };
                        page.block_size = block_size;
                        page.size_class = class as u8;
                        let page_start = page.page_start();
                        // SAFETY: `found_page` is a non-null interior pointer
                        // into this segment's page array (set in the scan loop
                        // above); `page_start` is its mapped backing region, so
                        // `initialize_free_list` writes only within the page, and
                        // `NonNull::new_unchecked(found_page)` is valid for the
                        // active-list insertion under the just-set `class`.
                        unsafe {
                            page.initialize_free_list::<P>(page_start, random_value);
                            self.push_active_page(NonNull::new_unchecked(found_page), class);
                        }
                        return found_page;
                    }

                    // Fallback to allocating another segment recursively
                    return unsafe { self.get_new_page::<P>(class) };
                } else {
                    self.fresh_segments += 1;
                    // Fresh segment initialization
                    // Safety: seg_ptr is valid, exclusive to this thread, and initialized.
                    // We set owner and insert it at the head of our owned segment list.
                    unsafe {
                        self.push_owned_segment::<P>(seg_ptr);
                        self.set_current_segment(Some(NonNull::new_unchecked(seg_ptr)));
                    }
                    self.next_page_index = 1; // page 0 is segment header
                }
            } else {
                return core::ptr::null_mut();
            }
        }

        debug_assert!(
            self.current_segment.is_some(),
            "get_new_page reached slicing path without a current segment"
        );
        // Safety: control only reaches here after the preceding branch either
        // observed a Some `current_segment` or installed one via
        // `allocate_segment`. The null-return path above precludes None.
        let seg = match self.current_segment {
            Some(s) => s.as_ptr(),
            None => unsafe { core::hint::unreachable_unchecked() },
        };
        // Safety: seg points to a valid Segment owned by us. We index into pages array.
        let page_ptr = unsafe { &mut (*seg).pages[self.next_page_index] as *mut Page };
        self.next_page_index += 1;

        // Safety: page_ptr points to a valid Page inside the segment.
        let page = unsafe { &mut *page_ptr };
        page.block_size = block_size;
        page.size_class = class as u8;

        let page_start = page.page_start();
        let random_value = if P::RANDOMIZE_ALLOCATION {
            self.next_random() ^ page_ptr as u64 ^ (class as u64).rotate_left(17)
        } else {
            0
        };
        unsafe {
            page.initialize_free_list::<P>(page_start, random_value);
        }

        // Prepend to the size class active pages list.
        unsafe {
            self.push_active_page(NonNull::new_unchecked(page_ptr), class);
        }

        self.fresh_pages += 1;
        page_ptr
    }
}

/// Pops segments from the pools/OS until one that policy `P` can own arrives,
/// returning policy-incompatible orphans to the orphan pool.
///
/// An orphan's live free chains are encoded under the segment's recorded
/// `free_list_encrypted` mode with the per-page keys already in its header,
/// while the owner-side hot paths (`pop_block`, local free) select encryption
/// statically from `P`. A thread may therefore adopt only an orphan whose
/// recorded mode matches `P::ENABLE_FREE_LIST_ENCRYPTION`; a mismatched orphan
/// is deferred on a local intrusive chain and pushed back to the orphan pool
/// for a matching-policy thread once a usable segment is found. Fresh and
/// pool-reinitialized segments (`free_list_encrypted == false`, zero live
/// allocations) are always usable: `push_owned_segment` keys them for `P`
/// before any chain is encoded.
///
/// Termination: each loop iteration either consumes one finite-pool segment
/// (free pool re-initializes, so `pages[1].block_size == 0` ends the loop;
/// each deferred orphan shrinks the orphan pool) or reaches the OS path,
/// which yields a fresh segment or `None`.
///
/// # Safety
///
/// Same contract as [`allocate_segment`]: the global pools must contain valid,
/// initialized `Segment`s. The returned segment (if any) is exclusively owned
/// by the caller.
#[inline(never)]
unsafe fn acquire_policy_compatible_segment<P: AllocPolicy, B: HasSegmentPool>(
) -> Option<*mut Segment> {
    let mut deferred: *mut Segment = core::ptr::null_mut();
    let chosen = loop {
        let Some(seg_ptr) = (unsafe { allocate_segment::<B>() }) else {
            break None;
        };
        // SAFETY: `seg_ptr` is the initialized, exclusively-owned segment just
        // returned by `allocate_segment`; `pages[1].block_size > 0`
        // distinguishes a previously-used orphan from a fresh segment, and
        // `free_list_encrypted` is its recorded chain-encoding mode.
        let incompatible_orphan = unsafe {
            (*seg_ptr).pages[1].block_size > 0
                && (*seg_ptr).free_list_encrypted != P::ENABLE_FREE_LIST_ENCRYPTION
        };
        if incompatible_orphan {
            // SAFETY: the segment is exclusively owned after the pop, so its
            // `next_free_segment` link is free to thread the deferral chain.
            unsafe { (*seg_ptr).next_free_segment = deferred };
            deferred = seg_ptr;
            continue;
        }
        break Some(seg_ptr);
    };
    while !deferred.is_null() {
        // SAFETY: `deferred` walks the exclusively-owned deferral chain built
        // above; each node is a valid orphan whose link is cleared before the
        // pool takes ownership back.
        unsafe {
            let next = (*deferred).next_free_segment;
            (*deferred).next_free_segment = core::ptr::null_mut();
            B::global_orphan_pool().push_unbounded(deferred);
            deferred = next;
        }
    }
    chosen
}
