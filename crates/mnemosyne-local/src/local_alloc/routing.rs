use super::page::{pop_page_free_block, try_reclaim_and_allocate};
use crate::local_alloc::ThreadAllocator;
use core::ptr::NonNull;
use mnemosyne_arena::{allocate_segment, HasSegmentPool};
use mnemosyne_core::constants::{PAGES_PER_SEGMENT, SEGMENT_SIZE};
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::size_class::{class_to_size, size_to_class};
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

            // 1. Check thread-local free list (hot fast path)
            if let Some(block) = page.free {
                // Safety: block points to a valid free Block inside the page.
                // We update page.free to the next block in the linked list.
                let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
                    let self_addr = page as *const Page as usize;
                    let segment_addr = self_addr & !(SEGMENT_SIZE - 1);
                    let segment = segment_addr as *mut Segment;
                    let page_index = page.index_in_segment();
                    unsafe { (*segment).keys[page_index] }
                } else {
                    0
                };
                unsafe {
                    page.free = (*block.as_ptr()).get_next::<P>(cookie);
                    page.increment_alloc_count();
                }
                return block.as_ptr() as *mut u8;
            }

            // Check lazy bump allocation
            if page.initialized_blocks < page.max_blocks() {
                let idx = page.initialized_blocks;
                page.initialized_blocks += 1;
                page.increment_alloc_count();
                let page_start = page.page_start();
                let block_ptr = unsafe { page_start.add(idx * page.block_size) };
                return block_ptr;
            }

            // 2. Reclaim batched cross-thread frees only after the local list is empty.
            // Safety: `page` is owned by this allocator and `try_reclaim_and_allocate`
            // upholds the `Page::reclaim_thread_free` contract on its behalf.
            if let Some(block) = unsafe { try_reclaim_and_allocate::<P>(page) } {
                return block.as_ptr() as *mut u8;
            }
        }

        // Outline the cold allocation path to keep alloc() small and fast.
        // Safety: Cold allocation path request is routed safely within bounds.
        unsafe { self.alloc_cold::<P>(class) }
    }

    /// Allocates a block of memory of the given size.
    ///
    /// Returns null if the size is not a small class or if allocation fails.
    ///
    /// # Safety
    ///
    /// This method is unsafe because it works with raw pointers and handles manual memory layouts.
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
            if let Some(block) = try_reclaim_and_allocate::<P>(active_page) {
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
            if let Some(block) = active_page.free {
                let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
                    let self_addr = active_page as *const Page as usize;
                    let segment_addr = self_addr & !(SEGMENT_SIZE - 1);
                    let segment = segment_addr as *mut Segment;
                    let page_index = active_page.index_in_segment();
                    unsafe { (*segment).keys[page_index] }
                } else {
                    0
                };
                unsafe {
                    active_page.free = (*block.as_ptr()).get_next::<P>(cookie);
                    active_page.increment_alloc_count();
                }
                return block.as_ptr() as *mut u8;
            }
            if active_page.initialized_blocks < active_page.max_blocks() {
                let idx = active_page.initialized_blocks;
                active_page.initialized_blocks += 1;
                active_page.increment_alloc_count();
                let page_start = active_page.page_start();
                let block_ptr = unsafe { page_start.add(idx * active_page.block_size) };
                return block_ptr;
            }
            if let Some(block) = unsafe { try_reclaim_and_allocate::<P>(active_page) } {
                return block.as_ptr() as *mut u8;
            }
        }

        // 2. Check if any page in full_pages has reclaimed cross-thread frees!
        // We only check pages that actually have pending cross-thread frees.
        // Also limit loop to 128 pages to bound search latency under threaded saturation.
        let mut curr_opt = unsafe { *self.full_pages.get_unchecked(class) };
        let mut checked = 0;
        while let Some(mut page_ptr) = curr_opt {
            if checked >= 128 {
                break;
            }
            checked += 1;
            let page = page_ptr.as_mut();
            // Safety: `page` is owned by this allocator.
            if let Some(block) = try_reclaim_and_allocate::<P>(page) {
                if page.alloc_count < page.max_blocks() {
                    // Page is no longer full! Move it back to active list.
                    // Safety: page_ptr and class are valid.
                    unsafe {
                        let _ = self.move_full_page_to_active(page_ptr, class);
                    }
                }
                return block.as_ptr() as *mut u8;
            }
            curr_opt = page.next_page;
        }

        // 3. Allocate a brand new page
        // Safety: get_new_page is called to retrieve a page.
        let new_page_ptr = self.get_new_page::<P>(class);
        if new_page_ptr.is_null() {
            return core::ptr::null_mut();
        }
        self.page_refills += 1;

        // Safety: new_page_ptr is valid and points to a Page inside a segment owned by us.
        let page = &mut *new_page_ptr;
        // Safety: `get_new_page` guarantees a freshly initialized page whose
        // `initialize_free_list` populated `free` with at least one block.
        let block = pop_page_free_block::<P>(page);

        page.increment_alloc_count();

        // If it becomes full immediately, move to full list
        if page.alloc_count == page.max_blocks() {
            // Safety: new_page_ptr and class are valid.
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
            // Safety: allocate_segment allocates a new segment from the OS/pool.
            if let Some(seg_ptr) = unsafe { allocate_segment::<B>() } {
                // Determine if this is an orphaned segment vs a fresh/reinitialized segment.
                // An orphaned segment has pages[1].block_size > 0.
                let is_orphan = unsafe { (*seg_ptr).pages[1].block_size > 0 };

                if is_orphan {
                    self.orphan_segments_adopted += 1;
                    let mut found_page: *mut Page = core::ptr::null_mut();
                    // Safety: We claim ownership of this orphaned segment. We scan and register pages.
                    unsafe {
                        self.push_owned_segment::<P>(seg_ptr);

                        self.set_current_segment(Some(NonNull::new_unchecked(seg_ptr)));
                        self.next_page_index = PAGES_PER_SEGMENT;

                        for i in 1..PAGES_PER_SEGMENT {
                            let page_ptr = &mut (*seg_ptr).pages[i] as *mut Page;
                            let page = &mut *page_ptr;

                            if page.block_size > 0 {
                                // Reclaim cross-thread frees to get accurate count.
                                let reclaimed = page.reclaim_thread_free::<P>();
                                if reclaimed > 0 {
                                    super::record_cross_thread_reclaimed(reclaimed);
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
                        // Safety: initializing free list for the repurposed page
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
