//! The thread-local allocator cache managing fast-path operations.

use core::marker::PhantomData;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_arena::{allocate_segment, deallocate_segment, HasSegmentPool};
use mnemosyne_backend::DefaultBackend;
use mnemosyne_core::constants::{NUM_SIZE_CLASSES, PAGES_PER_SEGMENT, PAGE_SIZE};
use mnemosyne_core::size_class::{class_to_size, size_to_class};
use mnemosyne_core::types::{Page, Segment, SegmentOwner};

static CROSS_THREAD_RECLAIMED_BLOCKS: AtomicUsize = AtomicUsize::new(0);

/// Occupancy counters for a single size class in the current thread allocator.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SizeClassOccupancy {
    pub active_pages: usize,
    pub empty_pages: usize,
    pub live_allocations: usize,
    pub total_slots: usize,
}

/// Snapshot of the current thread-local allocator state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadAllocatorStats {
    pub current_thread_live_allocations: usize,
    pub current_thread_owned_segments: usize,
    pub cross_thread_reclaimed_blocks: usize,
    pub page_refills: usize,
    pub recycled_pages: usize,
    pub fresh_pages: usize,
    pub fresh_segments: usize,
    pub orphan_segments_adopted: usize,
    pub recycle_sweeps: usize,
    pub size_class_occupancy: [SizeClassOccupancy; NUM_SIZE_CLASSES],
}

impl Default for ThreadAllocatorStats {
    fn default() -> Self {
        Self {
            current_thread_live_allocations: 0,
            current_thread_owned_segments: 0,
            cross_thread_reclaimed_blocks: 0,
            page_refills: 0,
            recycled_pages: 0,
            fresh_pages: 0,
            fresh_segments: 0,
            orphan_segments_adopted: 0,
            recycle_sweeps: 0,
            size_class_occupancy: [SizeClassOccupancy::default(); NUM_SIZE_CLASSES],
        }
    }
}

#[inline]
fn record_cross_thread_reclaimed(count: usize) {
    CROSS_THREAD_RECLAIMED_BLOCKS.fetch_add(count, Ordering::Relaxed);
}

/// Pops the head block from an initialized page-local free list.
///
/// # Safety
///
/// `page.free` must be `Some`; callers establish this through an existing
/// local free list, a successful `Page::reclaim_thread_free`, or
/// `Page::initialize_free_list`.
#[inline(always)]
unsafe fn pop_page_free_block(page: &mut Page) -> NonNull<mnemosyne_core::types::Block> {
    debug_assert!(
        page.free.is_some(),
        "page {} free list empty before pop; block_size={}, alloc_count={}, max_blocks={}",
        page.page_index,
        page.block_size,
        page.alloc_count,
        page.max_blocks
    );
    let block = match page.free {
        Some(block) => block,
        None => unsafe { core::hint::unreachable_unchecked() },
    };
    unsafe {
        page.free = (*block.as_ptr()).next;
    }
    block
}

/// Unlinks the page identified by `target` from the singly-linked list
/// whose head is stored in `head_slot`.
///
/// Returns `true` when the target was found and removed (and its
/// `next_page` cleared), or `false` when the target is not in this list.
/// Callers that maintain pages in two parallel lists (active and full) use
/// the boolean result to short-circuit the second walk.
///
/// The function walks the list linearly and mutates exactly three
/// pointers in the success path: the previous node's `next_page` (or the
/// list head if the target is the first node), and the target's
/// `next_page` (cleared so the unlinked node is detached). No pointer is
/// touched on the not-found path.
///
/// # Safety
///
/// Every node reachable from `*head_slot` must be a live `Page` owned by
/// the caller's allocator, and `target` must either be inside this list
/// or be a pointer that the caller still owns (so the `next_page` clear
/// on a found node is well-defined). The list is traversed by raw
/// pointer; the caller must not concurrently mutate the list.
#[inline]
unsafe fn unlink_page_from_list(head_slot: &mut Option<NonNull<Page>>, target: *mut Page) -> bool {
    let mut prev: Option<NonNull<Page>> = None;
    let mut curr = *head_slot;
    while let Some(curr_ptr) = curr {
        if curr_ptr.as_ptr() == target {
            // Safety: `target` is a live page node and the caller owns its
            // next_page field; the relink writes a single pointer slot
            // (either the previous node's next_page or the head).
            unsafe {
                let next = (*target).next_page;
                if let Some(mut prev_ptr) = prev {
                    prev_ptr.as_mut().next_page = next;
                } else {
                    *head_slot = next;
                }
                (*target).next_page = None;
            }
            return true;
        }
        prev = Some(curr_ptr);
        // Safety: `curr_ptr` is a live page node held by this list.
        curr = unsafe { curr_ptr.as_ref().next_page };
    }
    false
}

/// Reclaims any pending cross-thread frees on `page` and, if reclamation
/// added blocks to the local free list, pops one block and increments the
/// page's `alloc_count`.
///
/// Returns the popped block when reclamation succeeded, or `None` when
/// `page.thread_free` was empty.
///
/// The helper folds the three-step "drain remote frees, record telemetry,
/// allocate from the drained head" sequence used by `alloc` (active page),
/// `alloc_cold` (active-page recheck), and `alloc_cold` (full-page sweep)
/// into one site that is `#[inline(always)]` so monomorphization preserves
/// the prior hot-path codegen.
///
/// # Safety
///
/// Same contract as `Page::reclaim_thread_free`: the page must belong to
/// the allocator context performing the reconciliation and every block in
/// `page.thread_free` must belong to this page.
#[inline(always)]
unsafe fn try_reclaim_and_allocate(
    page: &mut Page,
) -> Option<NonNull<mnemosyne_core::types::Block>> {
    let reclaimed = unsafe { page.reclaim_thread_free() };
    if reclaimed == 0 {
        return None;
    }
    record_cross_thread_reclaimed(reclaimed);
    // Safety: `reclaim_thread_free` returning a nonzero count guarantees
    // that the drained chain is now linked onto `page.free`.
    let block = unsafe { pop_page_free_block(page) };
    page.alloc_count += 1;
    Some(block)
}

/// Thread-local cache for fast-path small allocations.
pub struct ThreadAllocator<B: HasSegmentPool = DefaultBackend> {
    /// Active pages per size class.
    pub active_pages: [Option<NonNull<Page>>; NUM_SIZE_CLASSES],
    /// Completely full pages per size class.
    pub full_pages: [Option<NonNull<Page>>; NUM_SIZE_CLASSES],
    /// Current segment being sliced into pages.
    pub current_segment: Option<NonNull<Segment>>,
    /// Index of the next page to slice in `current_segment`.
    pub next_page_index: usize,
    /// Head of the linked list of segments owned by this thread.
    pub owned_segments_head: *mut Segment,
    /// Number of successful cold-path page refills.
    pub page_refills: usize,
    /// Number of refills served by recycling an initialized empty page.
    pub recycled_pages: usize,
    /// Number of refills served by slicing a never-used page from the current segment.
    pub fresh_pages: usize,
    /// Number of fresh segments acquired by this allocator.
    pub fresh_segments: usize,
    /// Number of orphaned segments adopted by this allocator.
    pub orphan_segments_adopted: usize,
    /// Number of owned-segment sweeps made while searching for recyclable pages.
    pub recycle_sweeps: usize,
    /// Marker to bind the generic MemoryBackend parameter.
    pub _phantom: PhantomData<B>,
}

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Creates a new, uninitialized `ThreadAllocator`.
    pub const fn new() -> Self {
        Self {
            active_pages: [None; NUM_SIZE_CLASSES],
            full_pages: [None; NUM_SIZE_CLASSES],
            current_segment: None,
            next_page_index: 0,
            owned_segments_head: core::ptr::null_mut(),
            page_refills: 0,
            recycled_pages: 0,
            fresh_pages: 0,
            fresh_segments: 0,
            orphan_segments_adopted: 0,
            recycle_sweeps: 0,
            _phantom: PhantomData,
        }
    }

    /// Returns the process-wide number of blocks reclaimed from cross-thread free lists.
    pub fn cross_thread_reclaimed_blocks() -> usize {
        CROSS_THREAD_RECLAIMED_BLOCKS.load(Ordering::Relaxed)
    }

    /// Allocates a block of memory of the given size.
    ///
    /// Returns null if the size is not a small class or if allocation fails.
    ///
    /// # Safety
    ///
    /// This method is unsafe because it works with raw pointers and handles manual memory layouts.
    #[inline(always)]
    pub unsafe fn alloc(&mut self, size: usize) -> *mut u8 {
        let class = match size_to_class(size) {
            Some(c) => c,
            None => return core::ptr::null_mut(),
        };

        if let Some(mut page_ptr) = self.active_pages[class] {
            // Safety: page_ptr points to a valid Page structure inside an active segment owned by us.
            let page = unsafe { page_ptr.as_mut() };

            // 1. Check thread-local free list (hot fast path)
            if let Some(block) = page.free {
                // Safety: block points to a valid free Block inside the page.
                // We update page.free to the next block in the linked list.
                unsafe {
                    page.free = (*block.as_ptr()).next;
                }
                page.alloc_count += 1;
                return block.as_ptr() as *mut u8;
            }

            // 2. Reclaim batched cross-thread frees only after the local list is empty.
            // Safety: `page` is owned by this allocator and `try_reclaim_and_allocate`
            // upholds the `Page::reclaim_thread_free` contract on its behalf.
            if let Some(block) = unsafe { try_reclaim_and_allocate(page) } {
                return block.as_ptr() as *mut u8;
            }
        }

        // Outline the cold allocation path to keep alloc() small and fast.
        // Safety: Cold allocation path request is routed safely within bounds.
        unsafe { self.alloc_cold(class) }
    }

    /// Returns true when `segment` is the active segment being sliced by this thread.
    #[inline(always)]
    pub fn is_current_segment(&self, segment: *mut Segment) -> bool {
        self.current_segment
            .map_or(false, |current| current.as_ptr() == segment)
    }

    /// Updates the active slicing segment marker.
    ///
    /// # Safety
    ///
    /// Any segment in `segment` and the previous `current_segment` must be
    /// owned exclusively by this allocator while the marker is updated.
    #[inline(always)]
    unsafe fn set_current_segment(&mut self, segment: Option<NonNull<Segment>>) {
        if self.current_segment == segment {
            return;
        }
        if let Some(current) = self.current_segment {
            unsafe {
                (*current.as_ptr()).is_current = false;
            }
        }
        if let Some(next) = segment {
            unsafe {
                (*next.as_ptr()).is_current = true;
            }
        }
        self.current_segment = segment;
    }

    /// Returns a statistics snapshot for this thread allocator.
    pub fn stats(&self) -> ThreadAllocatorStats {
        let mut live_allocations = 0;
        let mut owned_segments = 0;
        let mut size_class_occupancy = [SizeClassOccupancy::default(); NUM_SIZE_CLASSES];
        let mut segment = self.owned_segments_head;
        while !segment.is_null() {
            owned_segments += 1;
            // Safety: segment points to a valid initialized segment owned by this thread.
            // We traverse the pages inside it to collect allocations statistics.
            unsafe {
                for page_index in 1..PAGES_PER_SEGMENT {
                    let page = &(*segment).pages[page_index];
                    live_allocations += page.alloc_count;
                    if page.block_size > 0 {
                        if let Some(class) = size_to_class(page.block_size) {
                            let occupancy = &mut size_class_occupancy[class];
                            occupancy.active_pages += 1;
                            if page.alloc_count == 0 {
                                occupancy.empty_pages += 1;
                            }
                            occupancy.live_allocations += page.alloc_count;
                            occupancy.total_slots += page.max_blocks;
                        }
                    }
                }
                segment = (*segment).next_owned_segment;
            }
        }

        ThreadAllocatorStats {
            current_thread_live_allocations: live_allocations,
            current_thread_owned_segments: owned_segments,
            cross_thread_reclaimed_blocks: CROSS_THREAD_RECLAIMED_BLOCKS.load(Ordering::Relaxed),
            page_refills: self.page_refills,
            recycled_pages: self.recycled_pages,
            fresh_pages: self.fresh_pages,
            fresh_segments: self.fresh_segments,
            orphan_segments_adopted: self.orphan_segments_adopted,
            recycle_sweeps: self.recycle_sweeps,
            size_class_occupancy,
        }
    }

    /// Cold path for allocating a block when active pages are full.
    ///
    /// Marked as `#[inline(never)]` to prevent pollution of instruction cache.
    #[inline(never)]
    unsafe fn alloc_cold(&mut self, class: usize) -> *mut u8 {
        // 1. Move the current active page to full_pages if it is indeed full.
        if let Some(mut active_ptr) = self.active_pages[class] {
            let active_page = active_ptr.as_mut();
            // Safety: `active_page` is owned by this allocator.
            if let Some(block) = try_reclaim_and_allocate(active_page) {
                return block.as_ptr() as *mut u8;
            }
            if active_page.free.is_none() {
                // The page is truly full! Move it to full_pages.
                self.active_pages[class] = active_page.next_page;
                active_page.next_page = self.full_pages[class];
                self.full_pages[class] = Some(active_ptr);
            }
        }

        // 2. Check if any page in full_pages has reclaimed cross-thread frees!
        // We only check pages that actually have pending cross-thread frees.
        // Also limit loop to 8 pages to bound search latency under threaded saturation.
        let mut prev: Option<NonNull<Page>> = None;
        let mut curr_opt = self.full_pages[class];
        let mut checked = 0;
        while let Some(mut page_ptr) = curr_opt {
            if checked >= 8 {
                break;
            }
            checked += 1;
            let page = page_ptr.as_mut();
            // Skip pages whose remote-free queue is empty without touching
            // metadata; saves an atomic swap on every iteration that has no
            // pending remote frees.
            if !page.thread_free.is_empty() {
                // Safety: `page` is owned by this allocator.
                if let Some(block) = try_reclaim_and_allocate(page) {
                    if page.alloc_count < page.max_blocks {
                        // Page is no longer full! Move it back to active list.
                        // Safety: page_ptr and class are valid.
                        if let Some(mut p) = prev {
                            p.as_mut().next_page = page.next_page;
                        } else {
                            self.full_pages[class] = page.next_page;
                        }
                        page.next_page = self.active_pages[class];
                        self.active_pages[class] = Some(page_ptr);
                    }
                    return block.as_ptr() as *mut u8;
                }
            }
            prev = Some(page_ptr);
            curr_opt = page.next_page;
        }

        // 3. Allocate a brand new page
        // Safety: get_new_page is called to retrieve a page.
        let new_page_ptr = self.get_new_page(class);
        if new_page_ptr.is_null() {
            return core::ptr::null_mut();
        }
        self.page_refills += 1;

        // Safety: new_page_ptr is valid and points to a Page inside a segment owned by us.
        let page = &mut *new_page_ptr;
        // Safety: `get_new_page` guarantees a freshly initialized page whose
        // `initialize_free_list` populated `free` with at least one block.
        let block = pop_page_free_block(page);

        page.alloc_count += 1;

        // If it becomes full immediately, move to full list
        if page.alloc_count == page.max_blocks {
            // Safety: new_page_ptr and class are valid.
            self.active_pages[class] = page.next_page;
            page.next_page = self.full_pages[class];
            self.full_pages[class] = Some(NonNull::new_unchecked(new_page_ptr));
        }
        block.as_ptr() as *mut u8
    }

    /// Obtains a new page for the given size class.
    ///
    /// # Safety
    ///
    /// Accesses and modifies segment pointers.
    unsafe fn get_new_page(&mut self, class: usize) -> *mut Page {
        let block_size = class_to_size(class);

        // Prefer never-used pages in the current segment. Recycle sweeps are only
        // needed after the active segment has no remaining unsliced pages.
        if self.current_segment.is_none() || self.next_page_index >= PAGES_PER_SEGMENT {
            self.recycle_sweeps += 1;
            // Safety: try_recycle_page sweeps owned segments and unlinks/relinks pages.
            if let Some(recycled_page) = unsafe { self.try_recycle_page(class) } {
                self.recycled_pages += 1;
                return recycled_page;
            }

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
                        (*seg_ptr).owner = SegmentOwner::from_ptr(self as *mut ThreadAllocator<B>);
                        (*seg_ptr).next_owned_segment = self.owned_segments_head;
                        self.owned_segments_head = seg_ptr;

                        self.set_current_segment(Some(NonNull::new_unchecked(seg_ptr)));
                        self.next_page_index = PAGES_PER_SEGMENT;

                        for i in 1..PAGES_PER_SEGMENT {
                            let page_ptr = &mut (*seg_ptr).pages[i] as *mut Page;
                            let page = &mut *page_ptr;

                            if page.block_size > 0 {
                                // Reclaim cross-thread frees to get accurate count.
                                let reclaimed = page.reclaim_thread_free();
                                if reclaimed > 0 {
                                    record_cross_thread_reclaimed(reclaimed);
                                }

                                if page.alloc_count > 0 {
                                    if page.alloc_count < page.max_blocks {
                                        if let Some(pg_class) = size_to_class(page.block_size) {
                                            page.next_page = self.active_pages[pg_class];
                                            self.active_pages[pg_class] =
                                                Some(NonNull::new_unchecked(page_ptr));
                                        }
                                    } else {
                                        if let Some(pg_class) = size_to_class(page.block_size) {
                                            page.next_page = self.full_pages[pg_class];
                                            self.full_pages[pg_class] =
                                                Some(NonNull::new_unchecked(page_ptr));
                                        }
                                    }
                                } else if found_page.is_null() {
                                    found_page = page_ptr;
                                }
                            } else if found_page.is_null() {
                                found_page = page_ptr;
                            }
                        }
                    }

                    if !found_page.is_null() {
                        let page = unsafe { &mut *found_page };
                        page.block_size = block_size;
                        let page_start =
                            unsafe { (seg_ptr as *mut u8).add(page.page_index * PAGE_SIZE) };
                        // Safety: initializing free list for the repurposed page
                        unsafe {
                            page.initialize_free_list(page_start);
                            page.next_page = self.active_pages[class];
                            self.active_pages[class] = Some(NonNull::new_unchecked(found_page));
                        }
                        return found_page;
                    }

                    // Fallback to allocating another segment recursively
                    return unsafe { self.get_new_page(class) };
                } else {
                    self.fresh_segments += 1;
                    // Fresh segment initialization
                    // Safety: seg_ptr is valid, exclusive to this thread, and initialized.
                    // We set owner and insert it at the head of our owned segment list.
                    unsafe {
                        (*seg_ptr).owner = SegmentOwner::from_ptr(self as *mut ThreadAllocator<B>);
                        (*seg_ptr).next_owned_segment = self.owned_segments_head;
                        self.owned_segments_head = seg_ptr;
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

        let page_start = unsafe { (seg as *mut u8).add(page.page_index * PAGE_SIZE) };
        unsafe {
            page.initialize_free_list(page_start);
        }

        // Prepend to the size class active pages list.
        page.next_page = self.active_pages[class];

        // Safety: page_ptr is a valid initialized page pointer.
        unsafe {
            self.active_pages[class] = Some(NonNull::new_unchecked(page_ptr));
        }

        self.fresh_pages += 1;
        page_ptr
    }

    /// Scans owned segments for an idle, empty page to recycle for a different size class.
    ///
    /// # Safety
    ///
    /// Sweeps raw segment metadata and unlinks/re-links page nodes.
    unsafe fn try_recycle_page(&mut self, class: usize) -> Option<*mut Page> {
        let block_size = class_to_size(class);
        let mut curr_seg = self.owned_segments_head;

        while !curr_seg.is_null() {
            // Safety: curr_seg is a valid pointer to a segment owned by us.
            unsafe {
                for i in 1..PAGES_PER_SEGMENT {
                    let pg = &mut (*curr_seg).pages[i];

                    // Reclaim thread_free first to get an accurate count
                    if !pg.thread_free.is_empty() {
                        let reclaimed = pg.reclaim_thread_free();
                        if reclaimed > 0 {
                            record_cross_thread_reclaimed(reclaimed);
                        }
                    }

                    // Recycle the page if it has zero active allocations and is already initialized
                    if pg.alloc_count == 0 && pg.block_size > 0 {
                        debug_assert!(
                            size_to_class(pg.block_size).is_some(),
                            "recyclable page block_size {} has no size class",
                            pg.block_size
                        );
                        // Safety: `pg.block_size` was previously assigned from
                        // `class_to_size(class)` for `class < NUM_SIZE_CLASSES`,
                        // so its inverse mapping always resolves.
                        let old_class = match size_to_class(pg.block_size) {
                            Some(c) => c,
                            None => core::hint::unreachable_unchecked(),
                        };
                        if pg.block_size != block_size {
                            // Unlink from old size class active list.
                            self.unlink_page(pg as *mut Page, old_class);

                            // Re-initialize for new size class.
                            pg.block_size = block_size;
                            let page_start = (curr_seg as *mut u8).add(pg.page_index * PAGE_SIZE);
                            pg.initialize_free_list(page_start);

                            // Prepend to new active list.
                            pg.next_page = self.active_pages[class];
                            self.active_pages[class] =
                                Some(NonNull::new_unchecked(pg as *mut Page));
                        }
                        return Some(pg as *mut Page);
                    }
                }
                curr_seg = (*curr_seg).next_owned_segment;
            }
        }
        None
    }

    /// Tries to reclaim a segment if it has zero active allocations.
    ///
    /// # Safety
    ///
    /// Accesses and modifies page and segment lists.
    pub unsafe fn try_reclaim_segment(&mut self, segment: *mut Segment) {
        if self
            .current_segment
            .map_or(false, |current| current.as_ptr() == segment)
        {
            return;
        }

        let mut total_allocations = 0;
        // Safety: segment is a valid pointer to a segment owned by us.
        unsafe {
            for i in 1..PAGES_PER_SEGMENT {
                let pg = &mut (*segment).pages[i];

                // Reclaim any cross-thread deallocations to get accurate alloc_count.
                let reclaimed = pg.reclaim_thread_free();
                if reclaimed > 0 {
                    record_cross_thread_reclaimed(reclaimed);
                }
                total_allocations += pg.alloc_count;
            }
        }

        // If the segment is completely empty, and it is not the only segment owned by this thread,
        // reclaim it immediately back to the pool.
        // Safety: segment is valid and owner field dereference is safe.
        let is_only_segment = self.owned_segments_head == segment
            && unsafe { (*segment).next_owned_segment.is_null() };
        if total_allocations == 0 && !is_only_segment {
            // Safety: segment is valid. We unlink all its pages from their size class lists.
            unsafe {
                for i in 1..PAGES_PER_SEGMENT {
                    let pg = &mut (*segment).pages[i];
                    if pg.block_size > 0 {
                        if let Some(class) = size_to_class(pg.block_size) {
                            self.unlink_page(pg as *mut Page, class);
                        }
                    }
                }

                self.unlink_owned_segment(segment);
            }

            if self
                .current_segment
                .map_or(false, |p| p.as_ptr() == segment)
            {
                self.set_current_segment(None);
                self.next_page_index = 0;
            }

            // Safety: segment is unlinked and exclusive to us. We clear fields and deallocate.
            unsafe {
                (*segment).owner = SegmentOwner::NONE;
                (*segment).next_owned_segment = core::ptr::null_mut();
                deallocate_segment::<B>(segment);
            }
        }
    }

    /// Helper to unlink a page specifically from the full pages list of a class.
    #[inline]
    #[must_use]
    pub(crate) unsafe fn unlink_full_page(&mut self, page_ptr: *mut Page, class: usize) -> bool {
        // Safety: `full_pages[class]` is the head of a singly-linked page
        // list owned by this allocator, and `page_ptr` is checked against
        // every node before any field write.
        unsafe { unlink_page_from_list(&mut self.full_pages[class], page_ptr) }
    }

    /// Helper to unlink a page from the active pages or full pages list of a class.
    #[inline]
    pub(crate) unsafe fn unlink_page(&mut self, page_ptr: *mut Page, class: usize) {
        // Safety: both `active_pages[class]` and `full_pages[class]` are heads of
        // singly-linked page lists owned by this allocator. `page_ptr` is checked
        // against each node before any pointer field is mutated, so a stale
        // pointer cannot corrupt the surrounding nodes.
        let removed_from_active =
            unsafe { unlink_page_from_list(&mut self.active_pages[class], page_ptr) };
        if removed_from_active {
            return;
        }
        let _ = unsafe { unlink_page_from_list(&mut self.full_pages[class], page_ptr) };
    }

    /// Helper to unlink a segment from the owned segments list.
    #[inline]
    unsafe fn unlink_owned_segment(&mut self, segment: *mut Segment) {
        let mut prev: *mut Segment = core::ptr::null_mut();
        let mut curr = self.owned_segments_head;
        while !curr.is_null() {
            if curr == segment {
                // Safety: segment points to a valid Segment node. We adjust owned segments list pointers.
                unsafe {
                    if !prev.is_null() {
                        (*prev).next_owned_segment = (*segment).next_owned_segment;
                    } else {
                        self.owned_segments_head = (*segment).next_owned_segment;
                    }
                    (*segment).next_owned_segment = core::ptr::null_mut();
                }
                break;
            }
            prev = curr;
            // Safety: curr is a valid pointer in the owned segments chain.
            curr = unsafe { (*curr).next_owned_segment };
        }
    }
}

impl<B: HasSegmentPool> Drop for ThreadAllocator<B> {
    fn drop(&mut self) {
        // When thread exits, we must reclaim all owned segments.
        let mut curr = self.owned_segments_head;
        while !curr.is_null() {
            // Safety: curr is a valid pointer in the owned segments chain.
            // We traverse the pages inside it, pop all cross-thread frees, and either deallocate or orphan it.
            unsafe {
                let next = (*curr).next_owned_segment;

                let mut total_allocations = 0;
                for i in 1..PAGES_PER_SEGMENT {
                    let page = &mut (*curr).pages[i];
                    let reclaimed = page.reclaim_thread_free();
                    if reclaimed > 0 {
                        record_cross_thread_reclaimed(reclaimed);
                    }
                    total_allocations += page.alloc_count;
                }

                (*curr).owner = SegmentOwner::NONE;
                (*curr).is_current = false;
                (*curr).next_owned_segment = core::ptr::null_mut();

                if total_allocations == 0 {
                    deallocate_segment::<B>(curr);
                } else {
                    B::global_orphan_pool().push_unbounded(curr);
                }

                curr = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::NonNull;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use mnemosyne_core::types::Block;
    use mnemosyne_core::MemoryBackend;

    // A mock tracking memory backend to verify custom backend injection.
    struct MockBackend;
    static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
    static DEALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
    static MOCK_SEGMENT_POOL: mnemosyne_arena::GlobalSegmentPool =
        mnemosyne_arena::GlobalSegmentPool::new();
    static MOCK_ORPHAN_POOL: mnemosyne_arena::GlobalSegmentPool =
        mnemosyne_arena::GlobalSegmentPool::new();
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    impl MemoryBackend for MockBackend {
        unsafe fn allocate(size: usize) -> *mut u8 {
            ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
            // Safety: delegate to DefaultBackend
            unsafe { DefaultBackend::allocate(size) }
        }

        unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
            DEALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
            // Safety: delegate to DefaultBackend
            unsafe { DefaultBackend::deallocate(ptr, size) }
        }
    }

    impl HasSegmentPool for MockBackend {
        #[inline(always)]
        fn global_segment_pool() -> &'static mnemosyne_arena::GlobalSegmentPool {
            &MOCK_SEGMENT_POOL
        }

        #[inline(always)]
        fn global_orphan_pool() -> &'static mnemosyne_arena::GlobalSegmentPool {
            &MOCK_ORPHAN_POOL
        }
    }

    crate::impl_local_allocator_selector!(MockBackend);
    crate::impl_local_allocator_selector!(DefaultBackend);

    #[test]
    fn test_custom_backend_injection() {
        let _guard = TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");
        ALLOC_COUNT.store(0, Ordering::SeqCst);
        DEALLOC_COUNT.store(0, Ordering::SeqCst);

        // Verify that the code compiles with ThreadAllocator parameterized by MockBackend
        let mut alloc = ThreadAllocator::<MockBackend>::new();
        // Safety: alloc is initialized and valid.
        let ptr = unsafe { alloc.alloc(32) };
        assert!(!ptr.is_null(), "MockBackend small allocation failed");
        unsafe {
            crate::thread_free::<mnemosyne_core::StandardPolicy, MockBackend>(ptr);
        }

        // Verify large allocation directly calls MockBackend
        // Safety: size and align are valid.
        let large_ptr =
            unsafe { mnemosyne_arena::allocate_large_or_huge::<MockBackend>(1024 * 1024, 8) };
        assert!(!large_ptr.is_null(), "MockBackend large allocation failed");
        assert!(
            ALLOC_COUNT.load(Ordering::SeqCst) >= 1,
            "MockBackend allocate counter was {}",
            ALLOC_COUNT.load(Ordering::SeqCst)
        );

        // Safety: large_ptr points to huge allocation segment.
        unsafe {
            let seg = ((large_ptr as usize) & !(mnemosyne_core::constants::SEGMENT_SIZE - 1))
                as *mut Segment;
            let _released =
                mnemosyne_arena::deallocate_large_or_huge::<MockBackend>(large_ptr, seg);
        }
        assert!(
            DEALLOC_COUNT.load(Ordering::SeqCst) >= 1,
            "MockBackend deallocate counter was {}",
            DEALLOC_COUNT.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn test_page_recycling_different_classes() {
        let _guard = TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");
        let mut alloc = ThreadAllocator::<DefaultBackend>::new();

        // 1. Allocate a block of size class 0 (16 bytes now)
        // Safety: alloc is initialized and valid.
        let ptr1 = unsafe { alloc.alloc(16) };
        assert!(!ptr1.is_null(), "initial 16-byte allocation failed");

        // We should have 1 owned segment now
        let first_stats = alloc.stats();
        assert_eq!(first_stats.current_thread_owned_segments, 1);
        assert_eq!(first_stats.page_refills, 1);
        assert_eq!(first_stats.recycle_sweeps, 1);

        // Determine which page this block belongs to
        let segment_addr = (ptr1 as usize) & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr1 as usize - segment_addr) / mnemosyne_core::constants::PAGE_SIZE;

        // Safety: segment points to a valid segment containing pages.
        let page = unsafe { &mut (*segment).pages[page_index] };

        // 2. Free the block locally by modifying metadata as thread_free would.
        // Since we are not running through thread_free routing, we manually perform a local free.
        // Safety: block ptr is valid and exclusive. We set up page free list.
        unsafe {
            let block = ptr1 as *mut Block;
            (*block).next = page.free;
            page.free = Some(NonNull::new_unchecked(block));
            page.alloc_count = 0; // Page is now empty
        }

        // 3. Now allocate a block of a DIFFERENT size class, say class 1 (32 bytes).
        // Force current-segment exhaustion so the allocator must sweep owned segments,
        // find the empty page (which was class 0),
        // unlink it from class 0, re-initialize it for class 1, and reuse it.
        alloc.next_page_index = PAGES_PER_SEGMENT;
        // Safety: alloc is initialized and valid.
        let ptr2 = unsafe { alloc.alloc(32) };
        assert!(!ptr2.is_null(), "recycled 32-byte allocation failed");

        // Assert that allocation stayed within the allocator's bounded owned-segment set.
        assert!(
            alloc.stats().current_thread_owned_segments <= 2,
            "owned segment count exceeded bound: {}",
            alloc.stats().current_thread_owned_segments
        );

        // Verify that allocation reused the owned segment and produced a page for the target class.
        let segment_addr2 = (ptr2 as usize) & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
        let page_index2 = (ptr2 as usize - segment_addr2) / mnemosyne_core::constants::PAGE_SIZE;
        let page2 = unsafe { &(*segment).pages[page_index2] };
        let expected_class = size_to_class(32).expect("32 bytes is a small allocation");

        assert_eq!(segment_addr2, segment_addr);
        assert_eq!(size_to_class(page2.block_size), Some(expected_class));
        assert_eq!(page2.block_size, class_to_size(expected_class));
        assert!(
            page2.alloc_count > 0,
            "recycled page should hold at least one allocation but had {}",
            page2.alloc_count
        );
        unsafe {
            crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(ptr2);
        }

        let recycled_stats = alloc.stats();
        assert!(
            recycled_stats.page_refills >= 2,
            "expected at least 2 page refills after recycle, observed {}",
            recycled_stats.page_refills
        );
        assert!(
            recycled_stats.recycled_pages >= 1,
            "expected at least 1 recycled page after class change, observed {}",
            recycled_stats.recycled_pages
        );
        assert!(
            recycled_stats.recycle_sweeps >= recycled_stats.page_refills,
            "recycle sweeps {} should bound page refills {}",
            recycled_stats.recycle_sweeps,
            recycled_stats.page_refills,
        );
    }

    #[test]
    fn unlink_full_page_reports_found_status_without_mutating_missing_page() {
        let _guard = TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");
        let mut alloc = ThreadAllocator::<DefaultBackend>::new();
        let class = size_to_class(16).expect("16 bytes is a small allocation");
        let mut listed = Page::new(1);
        let mut missing = Page::new(2);
        let listed_ptr = NonNull::from(&mut listed);
        let missing_ptr = NonNull::from(&mut missing);
        alloc.full_pages[class] = Some(listed_ptr);

        let removed_missing = unsafe { alloc.unlink_full_page(missing_ptr.as_ptr(), class) };

        assert!(
            !removed_missing,
            "unlink_full_page reported removal for a page outside the full list"
        );
        assert_eq!(
            alloc.full_pages[class].map(NonNull::as_ptr),
            Some(listed_ptr.as_ptr())
        );
        assert_eq!(missing.next_page, None);

        let removed_listed = unsafe { alloc.unlink_full_page(listed_ptr.as_ptr(), class) };

        assert!(
            removed_listed,
            "unlink_full_page did not report removal for the listed page"
        );
        assert_eq!(alloc.full_pages[class], None);
        assert_eq!(listed.next_page, None);
    }

    #[test]
    fn test_snmalloc_message_passing() {
        let _guard = TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");
        use std::thread;

        // Purge global segment pool to ensure we must allocate from the OS.
        unsafe {
            mnemosyne_arena::purge_segment_pool::<DefaultBackend>();
            mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
        }

        let mut alloc_a = ThreadAllocator::<DefaultBackend>::new();
        // Safety: alloc_a is initialized and valid.
        let ptr = unsafe { alloc_a.alloc(32) };
        assert!(
            !ptr.is_null(),
            "producer allocation for cross-thread free failed"
        );

        let ptr_usize = ptr as usize;

        // Verify that another thread can free A's block through the owning page queue.
        let handle = thread::spawn(move || {
            // Safety: freeing block allocated by A
            unsafe {
                crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(
                    ptr_usize as *mut u8,
                );
            }
        });
        handle.join().expect("cross-thread free worker panicked");

        let mut reclaimed_remote_free = false;
        let segment_addr = (ptr as usize) & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr as usize - segment_addr) / mnemosyne_core::constants::PAGE_SIZE;
        let max_blocks = unsafe { (*segment).pages[page_index].max_blocks };
        for _ in 0..max_blocks {
            // Safety: alloc_a is valid.
            let ptr2 = unsafe { alloc_a.alloc(32) };
            assert!(
                !ptr2.is_null(),
                "reclaim probe allocation failed before reclaiming remote free"
            );
            if ptr2 == ptr {
                reclaimed_remote_free = true;
                break;
            }
        }

        assert!(
            reclaimed_remote_free,
            "cross-thread freed block was not reclaimed after {} small allocations",
            max_blocks
        );
    }

    #[test]
    fn test_orphan_segment_reuse() {
        let _guard = TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");
        use std::sync::mpsc;
        use std::thread;

        unsafe {
            mnemosyne_arena::purge_segment_pool::<DefaultBackend>();
            mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
        }

        let (tx, rx) = mpsc::channel();

        // Thread A allocates a block and exits
        thread::spawn(move || {
            let mut alloc_a = ThreadAllocator::<DefaultBackend>::new();
            // Safety: alloc_a is valid.
            let ptr = unsafe { alloc_a.alloc(32) };
            assert!(!ptr.is_null(), "orphan producer allocation failed");
            tx.send(ptr as usize)
                .expect("orphan producer failed to send live allocation pointer");
        })
        .join()
        .expect("orphan producer thread panicked");

        let live_ptr = rx
            .recv()
            .expect("orphan producer did not send live allocation pointer")
            as *mut u8;

        // Thread B allocates a block. It should reuse the orphaned segment from A!
        let mut alloc_b = ThreadAllocator::<DefaultBackend>::new();
        // Safety: alloc_b is valid.
        let ptr_b = unsafe { alloc_b.alloc(64) };
        assert!(!ptr_b.is_null(), "orphan consumer allocation failed");

        // Assert that B reused the orphaned segment: current owned segments must be 1, not 2!
        assert_eq!(alloc_b.stats().current_thread_owned_segments, 1);

        // Free the allocations
        // Safety: pointers are valid and exclusive.
        unsafe {
            crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(live_ptr);
            crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(ptr_b);
        }
    }
}
