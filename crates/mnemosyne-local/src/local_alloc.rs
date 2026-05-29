//! The thread-local allocator cache managing fast-path operations.

use core::marker::PhantomData;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_arena::{allocate_segment, deallocate_segment, HasSegmentPool};
use mnemosyne_backend::DefaultBackend;
use mnemosyne_core::constants::{
    NUM_SIZE_CLASSES, PAGES_PER_SEGMENT, PAGE_SHIFT, PAGE_SIZE, SEGMENT_SIZE,
};
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::size_class::{class_to_size, size_to_class};
use mnemosyne_core::types::{Page, Segment, SegmentOwner};

std::thread_local! {
    static TLS_SEED: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

#[inline(always)]
fn get_tls_seed() -> usize {
    TLS_SEED.with(|cell| {
        let val = cell.get();
        if val == 0 {
            use std::hash::{BuildHasher, Hasher};
            let state = std::collections::hash_map::RandomState::new();
            let mut hasher = state.build_hasher();
            hasher.write_usize(0);
            let mut seed = hasher.finish() as usize;
            if seed == 0 {
                seed = 0xdeadbeeffacefeed;
            }
            cell.set(seed);
            seed
        } else {
            val
        }
    })
}

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
unsafe fn pop_page_free_block<P: AllocPolicy>(
    page: &mut Page,
) -> NonNull<mnemosyne_core::types::Block> {
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
unsafe fn try_reclaim_and_allocate<P: AllocPolicy>(
    page: &mut Page,
) -> Option<NonNull<mnemosyne_core::types::Block>> {
    let reclaimed = unsafe { page.reclaim_thread_free::<P>() };
    if reclaimed == 0 {
        return None;
    }
    record_cross_thread_reclaimed(reclaimed);
    // Safety: `reclaim_thread_free` returning a nonzero count guarantees
    // that the drained chain is now linked onto `page.free`.
    let block = unsafe { pop_page_free_block::<P>(page) };
    page.alloc_count += 1;
    Some(block)
}

/// Thread-local cache for fast-path small allocations.
pub struct ThreadAllocator<B: HasSegmentPool = DefaultBackend> {
    /// Active pages per size class.
    pub active_pages: [Option<NonNull<Page>>; NUM_SIZE_CLASSES],
    /// Completely full pages per size class.
    pub full_pages: [Option<NonNull<Page>>; NUM_SIZE_CLASSES],
    /// Stack of empty/defragmented pages available for recycling.
    pub empty_pages: Option<NonNull<Page>>,
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
            empty_pages: None,
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
    pub unsafe fn alloc<P: AllocPolicy>(&mut self, size: usize) -> *mut u8 {
        let class = match size_to_class(size) {
            Some(c) => c,
            None => return core::ptr::null_mut(),
        };

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
                }
                page.alloc_count += 1;
                return block.as_ptr() as *mut u8;
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
                        let class = page.size_class;
                        let occupancy = &mut size_class_occupancy[class];
                        occupancy.active_pages += 1;
                        if page.alloc_count == 0 {
                            occupancy.empty_pages += 1;
                        }
                        occupancy.live_allocations += page.alloc_count;
                        occupancy.total_slots += page.max_blocks;
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
    pub(crate) unsafe fn alloc_cold<P: AllocPolicy>(&mut self, class: usize) -> *mut u8 {
        // 1. Move the current active page to full_pages if it is indeed full.
        if let Some(mut active_ptr) = unsafe { *self.active_pages.get_unchecked(class) } {
            let active_page = active_ptr.as_mut();
            // Safety: `active_page` is owned by this allocator.
            if let Some(block) = try_reclaim_and_allocate::<P>(active_page) {
                return block.as_ptr() as *mut u8;
            }
            if active_page.free.is_none() {
                // The page is truly full! Move it to full_pages.
                unsafe {
                    *self.active_pages.get_unchecked_mut(class) = active_page.next_page;
                    active_page.next_page = *self.full_pages.get_unchecked(class);
                    *self.full_pages.get_unchecked_mut(class) = Some(active_ptr);
                }
            }
        }

        // 2. Check if any page in full_pages has reclaimed cross-thread frees!
        // We only check pages that actually have pending cross-thread frees.
        // Also limit loop to 8 pages to bound search latency under threaded saturation.
        let mut prev: Option<NonNull<Page>> = None;
        let mut curr_opt = unsafe { *self.full_pages.get_unchecked(class) };
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
                if let Some(block) = try_reclaim_and_allocate::<P>(page) {
                    if page.alloc_count < page.max_blocks {
                        // Page is no longer full! Move it back to active list.
                        // Safety: page_ptr and class are valid.
                        unsafe {
                            if let Some(mut p) = prev {
                                p.as_mut().next_page = page.next_page;
                            } else {
                                *self.full_pages.get_unchecked_mut(class) = page.next_page;
                            }
                            page.next_page = *self.active_pages.get_unchecked(class);
                            *self.active_pages.get_unchecked_mut(class) = Some(page_ptr);
                        }
                    }
                    return block.as_ptr() as *mut u8;
                }
            }
            prev = Some(page_ptr);
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

        page.alloc_count += 1;

        // If it becomes full immediately, move to full list
        if page.alloc_count == page.max_blocks {
            // Safety: new_page_ptr and class are valid.
            unsafe {
                *self.active_pages.get_unchecked_mut(class) = page.next_page;
                page.next_page = *self.full_pages.get_unchecked(class);
                *self.full_pages.get_unchecked_mut(class) =
                    Some(NonNull::new_unchecked(new_page_ptr));
            }
        }
        block.as_ptr() as *mut u8
    }

    /// Obtains a new page for the given size class.
    ///
    /// # Safety
    ///
    /// Accesses and modifies segment pointers.
    /// Obtains a new page for the given size class.
    ///
    /// # Safety
    ///
    /// Accesses and modifies segment pointers.
    unsafe fn get_new_page<P: AllocPolicy>(&mut self, class: usize) -> *mut Page {
        let block_size = class_to_size(class);

        // Check if there is an empty page in the defragmentation list first.
        if let Some(mut page_ptr) = self.empty_pages {
            unsafe {
                self.empty_pages = page_ptr.as_ref().next_page;
                let page = page_ptr.as_mut();
                page.next_page = None;

                let segment_addr = (page_ptr.as_ptr() as usize) & !(SEGMENT_SIZE - 1);
                let page_start = (segment_addr as *mut u8).add(page.page_index << PAGE_SHIFT);
                page.block_size = block_size;
                page.size_class = class;
                page.initialize_free_list::<P>(page_start);

                page.next_page = *self.active_pages.get_unchecked(class);
                *self.active_pages.get_unchecked_mut(class) = Some(page_ptr);
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
                                    record_cross_thread_reclaimed(reclaimed);
                                }

                                if page.alloc_count > 0 {
                                    let pg_class = page.size_class;
                                    if page.alloc_count < page.max_blocks {
                                        page.next_page = *self.active_pages.get_unchecked(pg_class);
                                        *self.active_pages.get_unchecked_mut(pg_class) =
                                            Some(NonNull::new_unchecked(page_ptr));
                                    } else {
                                        page.next_page = *self.full_pages.get_unchecked(pg_class);
                                        *self.full_pages.get_unchecked_mut(pg_class) =
                                            Some(NonNull::new_unchecked(page_ptr));
                                    }
                                } else if found_page.is_null() {
                                    found_page = page_ptr;
                                } else {
                                    page.next_page = self.empty_pages;
                                    self.empty_pages = Some(NonNull::new_unchecked(page_ptr));
                                }
                            } else if found_page.is_null() {
                                found_page = page_ptr;
                            } else {
                                page.next_page = self.empty_pages;
                                self.empty_pages = Some(NonNull::new_unchecked(page_ptr));
                            }
                        }
                    }

                    if !found_page.is_null() {
                        let page = unsafe { &mut *found_page };
                        page.block_size = block_size;
                        page.size_class = class;
                        let page_start =
                            unsafe { (seg_ptr as *mut u8).add(page.page_index << PAGE_SHIFT) };
                        // Safety: initializing free list for the repurposed page
                        unsafe {
                            page.initialize_free_list::<P>(page_start);
                            page.next_page = *self.active_pages.get_unchecked(class);
                            *self.active_pages.get_unchecked_mut(class) =
                                Some(NonNull::new_unchecked(found_page));
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
        page.size_class = class;

        let page_start = unsafe { (seg as *mut u8).add(page.page_index << PAGE_SHIFT) };
        unsafe {
            page.initialize_free_list::<P>(page_start);
        }

        // Prepend to the size class active pages list.
        unsafe {
            page.next_page = *self.active_pages.get_unchecked(class);
            *self.active_pages.get_unchecked_mut(class) = Some(NonNull::new_unchecked(page_ptr));
        }

        self.fresh_pages += 1;
        page_ptr
    }

    /// Tries to reclaim a segment if it has zero active allocations.
    ///
    /// # Safety
    ///
    /// Accesses and modifies page and segment lists.
    pub unsafe fn try_reclaim_segment(&mut self, segment: *mut Segment) -> bool {
        if self
            .current_segment
            .map_or(false, |current| current.as_ptr() == segment)
        {
            return false;
        }

        let mut total_allocations = 0;
        // Safety: segment is a valid pointer to a segment owned by us.
        unsafe {
            let dynamic_encrypted = (*segment).free_list_encrypted;
            for i in 1..PAGES_PER_SEGMENT {
                let pg = &mut (*segment).pages[i];

                // Reclaim any cross-thread deallocations to get accurate alloc_count.
                let reclaimed = pg.reclaim_thread_free_dynamic(dynamic_encrypted);
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
                        let class = pg.size_class;
                        self.unlink_page(pg as *mut Page, class);
                    }
                    self.unlink_empty_page(pg as *mut Page);
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
            true
        } else {
            false
        }
    }

    /// Helper to unlink a page specifically from the full pages list of a class.
    #[inline]
    #[must_use]
    pub(crate) unsafe fn unlink_full_page(&mut self, page_ptr: *mut Page, class: usize) -> bool {
        debug_assert!(class < NUM_SIZE_CLASSES);
        // Safety: `full_pages[class]` is the head of a singly-linked page
        // list owned by this allocator, and `page_ptr` is checked against
        // every node before any field write.
        unsafe { unlink_page_from_list(self.full_pages.get_unchecked_mut(class), page_ptr) }
    }

    /// Helper to unlink a page from the active pages or full pages list of a class.
    #[inline]
    pub(crate) unsafe fn unlink_page(&mut self, page_ptr: *mut Page, class: usize) {
        debug_assert!(class < NUM_SIZE_CLASSES);
        // Safety: both `active_pages[class]` and `full_pages[class]` are heads of
        // singly-linked page lists owned by this allocator. `page_ptr` is checked
        // against each node before any pointer field is mutated, so a stale
        // pointer cannot corrupt the surrounding nodes.
        let removed_from_active =
            unsafe { unlink_page_from_list(self.active_pages.get_unchecked_mut(class), page_ptr) };
        if removed_from_active {
            return;
        }
        let _ =
            unsafe { unlink_page_from_list(self.full_pages.get_unchecked_mut(class), page_ptr) };
    }

    /// Helper to unlink a page from the empty pages list.
    #[inline]
    pub(crate) unsafe fn unlink_empty_page(&mut self, page_ptr: *mut Page) -> bool {
        unsafe { unlink_page_from_list(&mut self.empty_pages, page_ptr) }
    }

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
    unsafe fn push_owned_segment<P: AllocPolicy>(&mut self, segment: *mut Segment) {
        // Safety: `segment` is exclusive to this allocator; the caller
        // guarantees it is unlinked, so overwriting its link fields and
        // relinking the current head is sound.
        unsafe {
            (*segment).owner = SegmentOwner::from_ptr(self as *mut ThreadAllocator<B>);
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
        let seed = get_tls_seed();
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
    unsafe fn unlink_owned_segment(&mut self, segment: *mut Segment) {
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
}

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Reclaims every segment owned by this thread cache back to the global
    /// pools, then clears the owned-segment chain so the operation is
    /// idempotent.
    ///
    /// This is the canonical thread-exit reclamation path. It is invoked by the
    /// `Drop` implementation for the default `std::thread_local!`-backed cache,
    /// and by the `#[thread_local]`-backed fast cache's exit sentinel (a
    /// `#[thread_local]` static does not run `Drop` on thread teardown, so the
    /// sentinel calls this method explicitly). Clearing the head after the
    /// sweep guarantees a second invocation is a no-op, which keeps both call
    /// sites safe even if they ever overlap.
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
                        record_cross_thread_reclaimed(reclaimed);
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
}

impl<B: HasSegmentPool> Drop for ThreadAllocator<B> {
    fn drop(&mut self) {
        self.reclaim_owned_segments();
    }
}

#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::NonNull;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use mnemosyne_core::policy::StandardPolicy;
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

    impl mnemosyne_arena::segment::pool::private::Sealed for MockBackend {}

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
        let ptr = unsafe { alloc.alloc::<StandardPolicy>(32) };
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

    /// Proves the `#[thread_local]` fast cache still reclaims a terminating
    /// thread's owned segments. A `#[thread_local]` static is not dropped on
    /// thread exit, so reclamation depends entirely on the exit sentinel
    /// (`ThreadExitReclaim`) armed on first allocation. The spawned thread
    /// allocates through the TLS path and exits without freeing; the live
    /// segment must therefore be orphaned into `MOCK_ORPHAN_POOL`. If the
    /// sentinel failed to fire the segment would leak and the pool would stay
    /// empty, failing the value-semantic assertion below.
    #[cfg(feature = "nightly_tls")]
    #[test]
    fn thread_exit_sentinel_reclaims_owned_segments_on_fast_tls_path() {
        let _guard = TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");

        // Drain any residue so the post-join count reflects only this thread.
        while let Some(seg) = MOCK_ORPHAN_POOL.pop() {
            // Safety: pooled segments are valid mappings owned by the pool.
            unsafe { mnemosyne_arena::deallocate_segment::<MockBackend>(seg) };
        }

        let handle = std::thread::spawn(|| {
            // Safety: a 32-byte/16-align request is a valid small allocation;
            // routing through the TLS path arms the exit sentinel and acquires
            // a segment owned by this thread. The block is intentionally not
            // freed so the owning segment is still live at thread exit.
            let ptr = unsafe {
                crate::thread_alloc::<mnemosyne_core::StandardPolicy, MockBackend>(32, 16)
            };
            assert!(!ptr.is_null(), "fast-TLS small allocation failed");
            ptr as usize
        });
        let block_addr = handle.join().expect("spawned allocator thread panicked");
        assert_ne!(block_addr, 0, "spawned thread produced a null allocation");

        // The exit sentinel must have orphaned the still-live owning segment.
        let mut reclaimed = 0usize;
        while let Some(seg) = MOCK_ORPHAN_POOL.pop() {
            reclaimed += 1;
            // Safety: pooled segments are valid mappings; release the mapping
            // (including the never-freed block) to avoid leaking the test's
            // owned segment beyond the assertion.
            unsafe { mnemosyne_arena::deallocate_segment::<MockBackend>(seg) };
        }
        assert!(
            reclaimed >= 1,
            "thread-exit sentinel did not reclaim the live owned segment; \
             orphan pool received {reclaimed} segments"
        );
    }

    #[test]
    fn thread_exit_reclaims_owned_segments_on_selected_tls_path() {
        let _guard = TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");

        // Drain any residue so the post-join count reflects only this thread.
        while let Some(seg) = MOCK_ORPHAN_POOL.pop() {
            // Safety: pooled segments are valid mappings owned by the pool.
            unsafe { mnemosyne_arena::deallocate_segment::<MockBackend>(seg) };
        }

        let handle = std::thread::spawn(|| {
            // Safety: a 32-byte/16-align request is a valid small allocation;
            // routing through the TLS path arms the exit sentinel (or registers standard drop)
            // and acquires a segment owned by this thread. The block is intentionally not
            // freed so the owning segment is still live at thread exit.
            let ptr = unsafe {
                crate::thread_alloc::<mnemosyne_core::StandardPolicy, MockBackend>(32, 16)
            };
            assert!(!ptr.is_null(), "selected-TLS small allocation failed");
            ptr as usize
        });
        let block_addr = handle.join().expect("spawned allocator thread panicked");
        assert_ne!(block_addr, 0, "spawned thread produced a null allocation");

        // The exit sentinel or slot drop must have orphaned the still-live owning segment.
        let mut reclaimed = 0usize;
        while let Some(seg) = MOCK_ORPHAN_POOL.pop() {
            reclaimed += 1;
            // Safety: pooled segments are valid mappings; release the mapping
            // (including the never-freed block) to avoid leaking the test's
            // owned segment beyond the assertion.
            unsafe { mnemosyne_arena::deallocate_segment::<MockBackend>(seg) };
        }
        assert!(
            reclaimed >= 1,
            "thread-exit did not reclaim the live owned segment; \
             orphan pool received {reclaimed} segments"
        );
    }

    /// Verifies the intrusive doubly-linked owned-segments list splices any
    /// node out in place and in O(1) — without searching for a predecessor —
    /// while preserving the `prev`/`next` invariant across head, middle, and
    /// tail removals. This pins the correctness of `push_owned_segment` and the
    /// O(1) `unlink_owned_segment` that replaced the prior linear search.
    #[test]
    fn owned_segment_list_is_doubly_linked_and_unlinks_in_place() {
        let _guard = TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");
        let mut alloc = ThreadAllocator::<DefaultBackend>::new();

        // Three standalone segment headers. Alignment is irrelevant here: the
        // list helpers only touch the `prev`/`next` link fields, never the page
        // data, so heap-boxed storage is sufficient and avoids real OS mappings.
        let mut storage: [std::boxed::Box<core::mem::MaybeUninit<Segment>>; 3] = [
            std::boxed::Box::new(core::mem::MaybeUninit::uninit()),
            std::boxed::Box::new(core::mem::MaybeUninit::uninit()),
            std::boxed::Box::new(core::mem::MaybeUninit::uninit()),
        ];
        let mut seg = [core::ptr::null_mut::<Segment>(); 3];
        for (i, slot) in storage.iter_mut().enumerate() {
            let ptr = slot.as_mut_ptr();
            // Safety: `ptr` is a unique, writable, suitably sized allocation.
            unsafe {
                Segment::initialize(ptr, core::ptr::null_mut());
                alloc.push_owned_segment::<StandardPolicy>(ptr);
            }
            seg[i] = ptr;
        }

        // Pushing seg0, seg1, seg2 yields head: seg2 -> seg1 -> seg0.
        assert_eq!(
            alloc.owned_segments_head, seg[2],
            "head must be last pushed"
        );
        // Safety: all three pointers are live boxed segments linked above.
        unsafe {
            assert!((*seg[2]).prev_owned_segment.is_null());
            assert_eq!((*seg[2]).next_owned_segment, seg[1]);
            assert_eq!((*seg[1]).prev_owned_segment, seg[2]);
            assert_eq!((*seg[1]).next_owned_segment, seg[0]);
            assert_eq!((*seg[0]).prev_owned_segment, seg[1]);
            assert!((*seg[0]).next_owned_segment.is_null());
        }

        // Unlink the MIDDLE node (seg1): list becomes seg2 -> seg0 with no
        // predecessor search.
        // Safety: seg1 is a live node in this list.
        unsafe { alloc.unlink_owned_segment(seg[1]) };
        assert_eq!(alloc.owned_segments_head, seg[2]);
        // Safety: pointers remain live (storage outlives this scope).
        unsafe {
            assert_eq!((*seg[2]).next_owned_segment, seg[0]);
            assert_eq!((*seg[0]).prev_owned_segment, seg[2]);
            // The detached node carries no stale list pointers.
            assert!((*seg[1]).prev_owned_segment.is_null());
            assert!((*seg[1]).next_owned_segment.is_null());
        }

        // Unlink the HEAD node (seg2): list becomes seg0.
        // Safety: seg2 is the live head node.
        unsafe { alloc.unlink_owned_segment(seg[2]) };
        assert_eq!(alloc.owned_segments_head, seg[0]);
        // Safety: seg0 is the sole remaining live node.
        unsafe { assert!((*seg[0]).prev_owned_segment.is_null()) };

        // Unlink the TAIL/only node (seg0): list becomes empty.
        // Safety: seg0 is the live sole node.
        unsafe { alloc.unlink_owned_segment(seg[0]) };
        assert!(alloc.owned_segments_head.is_null(), "list must be empty");

        // `alloc` now owns no segments; its `Drop` reclamation is a no-op, so
        // the boxed storage is freed safely when `storage` drops here.
    }

    /// Proves `Page::index_in_segment` (address derivation) equals the stored
    /// `page_index` field for every page of a real `SEGMENT_ALIGN`-aligned
    /// segment. This pins the correctness of the derivation that lets a future
    /// change drop the stored `page_index` field and reclaim its 8 bytes for a
    /// doubly-linked `prev_page` back-pointer (O(1) page-list unlink) while
    /// keeping `Page` within one 64-byte cache line. A real segment from the
    /// backend is used because the derivation rounds the page address down to
    /// `SEGMENT_ALIGN`, which only holds for genuinely segment-aligned memory.
    #[test]
    fn page_index_field_matches_address_derivation() {
        let _guard = TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");

        // Safety: allocate a real segment-aligned segment from the backend.
        let seg =
            unsafe { allocate_segment::<DefaultBackend>() }.expect("segment allocation failed");
        assert_eq!(
            seg as usize % mnemosyne_core::constants::SEGMENT_ALIGN,
            0,
            "backend segment is not SEGMENT_ALIGN-aligned"
        );

        for i in 0..PAGES_PER_SEGMENT {
            // Safety: `seg` is a live initialized segment; page `i` is in bounds.
            let page = unsafe { &(*seg).pages[i] };
            assert_eq!(
                page.index_in_segment(),
                i,
                "address derivation disagrees with array position at page {i}"
            );
            assert_eq!(
                page.index_in_segment(),
                page.page_index,
                "address derivation disagrees with stored page_index at page {i}"
            );
        }

        // Safety: `seg` was returned by `allocate_segment` and is unaliased.
        unsafe { deallocate_segment::<DefaultBackend>(seg) };
    }

    /// Validates the singly-linked page-list splice helper `unlink_page_from_list`
    /// across head, middle, tail, and absent-target cases. Page nodes are held
    /// as fully raw allocations (`Box::into_raw`) so the test never interleaves
    /// `Box` and raw-pointer access — keeping it clean under Miri's Stacked
    /// Borrows checker, which has no FFI-backed allocator path to exercise this
    /// logic otherwise.
    #[test]
    fn unlink_page_from_list_splices_and_reports_membership() {
        let _guard = TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");

        // Three standalone page nodes owned as raw pointers for the duration.
        let p0 = std::boxed::Box::into_raw(std::boxed::Box::new(Page::new(1)));
        let p1 = std::boxed::Box::into_raw(std::boxed::Box::new(Page::new(2)));
        let p2 = std::boxed::Box::into_raw(std::boxed::Box::new(Page::new(3)));
        // Safety: p0/p1/p2 are unique live allocations; build head -> p0 -> p1 -> p2.
        let (n0, _n1, n2) = unsafe {
            let n0 = NonNull::new_unchecked(p0);
            let n1 = NonNull::new_unchecked(p1);
            let n2 = NonNull::new_unchecked(p2);
            (*p0).next_page = Some(n1);
            (*p1).next_page = Some(n2);
            (*p2).next_page = None;
            (n0, n1, n2)
        };
        let mut head = Some(n0);

        // Unlink the MIDDLE node: head -> p0 -> p2, p1 detached.
        // Safety: all nodes live; `p1` is in the list.
        assert!(unsafe { unlink_page_from_list(&mut head, p1) });
        assert_eq!(head, Some(n0));
        // Safety: nodes remain live.
        unsafe {
            assert_eq!((*p0).next_page, Some(n2));
            assert_eq!((*p1).next_page, None);
        }

        // Unlink the HEAD node: head -> p2.
        // Safety: `p0` is the head.
        assert!(unsafe { unlink_page_from_list(&mut head, p0) });
        assert_eq!(head, Some(n2));

        // Unlink an ABSENT node (p1 again): reports false, list untouched.
        // Safety: `p1` is no longer in the list.
        assert!(!unsafe { unlink_page_from_list(&mut head, p1) });
        assert_eq!(head, Some(n2));

        // Unlink the TAIL/only node: list empties.
        // Safety: `p2` is the sole node.
        assert!(unsafe { unlink_page_from_list(&mut head, p2) });
        assert!(head.is_none());

        // Reclaim the raw allocations.
        // Safety: each pointer came from `Box::into_raw` and is unaliased now.
        unsafe {
            drop(std::boxed::Box::from_raw(p0));
            drop(std::boxed::Box::from_raw(p1));
            drop(std::boxed::Box::from_raw(p2));
        }
    }

    /// Safety regression guard for the guard-free small-allocation fast path.
    ///
    /// The fast path borrows the thread cache without arming the re-entrancy
    /// guard, so it MUST still observe the busy bit set by an outer guarded
    /// borrow and decline — otherwise a same-thread re-entrant allocation
    /// (a custom/telemetry backend, or production tracing) would create a
    /// second `&mut ThreadAllocator` aliasing the live guarded borrow, which is
    /// undefined behavior. This test enters a guarded borrow and asserts that a
    /// nested `with_allocator_unguarded` returns `None`, and that the same call
    /// succeeds when no guard is held.
    #[test]
    fn unguarded_fast_path_rejects_reentrant_borrow() {
        use crate::LocalAllocatorSelector;
        let _guard = TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");

        let outer_saw_reentrant_none = MockBackend::with_allocator(|_outer| {
            // Inside the guarded borrow: is_allocating is set.
            // Safety: the probe closure performs no allocator re-entry.
            let reentrant =
                unsafe { MockBackend::with_allocator_unguarded(|_inner| 0xC0FFEE_usize) };
            reentrant.is_none()
        });
        assert_eq!(
            outer_saw_reentrant_none,
            Some(true),
            "unguarded fast path aliased a live guarded borrow instead of rejecting re-entry"
        );

        // With no guard held, the unguarded path is permitted and runs `f`.
        // Safety: the closure does not re-enter the allocator.
        let allowed = unsafe { MockBackend::with_allocator_unguarded(|_alloc| 7_usize) };
        assert_eq!(
            allowed,
            Some(7),
            "unguarded path must run the closure when no guard is held"
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
        let ptr1 = unsafe { alloc.alloc::<StandardPolicy>(16) };
        assert!(!ptr1.is_null(), "initial 16-byte allocation failed");

        // We should have 1 owned segment now
        let first_stats = alloc.stats();
        assert_eq!(first_stats.current_thread_owned_segments, 1);
        assert_eq!(first_stats.page_refills, 1);

        // Determine which page this block belongs to
        let ptr1_val = ptr1 as usize;
        let segment_addr = ptr1_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr1_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

        // Safety: segment points to a valid segment containing pages.
        let page = unsafe { &mut (*segment).pages[page_index] };

        // 2. Free the block locally by modifying metadata as thread_free would.
        // Since we are not running through thread_free routing, we manually perform a local free.
        // Safety: block ptr is valid and exclusive. We set up page free list.
        unsafe {
            let block = ptr1 as *mut Block;
            (*block).set_next::<StandardPolicy>(page.free, 0);
            page.free = Some(NonNull::new_unchecked(block));
            page.alloc_count = 0; // Page is now empty
            let class = page.size_class;
            alloc.unlink_page(page as *mut Page, class);
            page.next_page = alloc.empty_pages;
            alloc.empty_pages = Some(NonNull::new_unchecked(page as *mut Page));
        }

        // 3. Now allocate a block of a DIFFERENT size class, say class 1 (32 bytes).
        // Force current-segment exhaustion so the allocator must sweep owned segments,
        // find the empty page (which was class 0),
        // unlink it from class 0, re-initialize it for class 1, and reuse it.
        alloc.next_page_index = PAGES_PER_SEGMENT;
        // Safety: alloc is initialized and valid.
        let ptr2 = unsafe { alloc.alloc::<StandardPolicy>(32) };
        assert!(!ptr2.is_null(), "recycled 32-byte allocation failed");

        // Assert that allocation stayed within the allocator's bounded owned-segment set.
        assert!(
            alloc.stats().current_thread_owned_segments <= 2,
            "owned segment count exceeded bound: {}",
            alloc.stats().current_thread_owned_segments
        );

        // Verify that allocation reused the owned segment and produced a page for the target class.
        let ptr2_val = ptr2 as usize;
        let segment_addr2 = ptr2_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
        let page_index2 = (ptr2_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
        let page2 = unsafe { &(*segment).pages[page_index2] };
        let expected_class = size_to_class(32).expect("32 bytes is a small allocation");

        assert_eq!(segment_addr2, segment_addr);
        assert_eq!(page2.size_class, expected_class);
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
    }

    #[test]
    fn smallest_class_page_saturates_without_duplicate_or_early_refill() {
        // Filling one smallest-class (16-byte) page to capacity must drive
        // `alloc_count` up to `max_blocks` (4096 for a 64 KiB page) with
        // every pointer distinct and non-null. A counter/list defect would
        // surface here as a repeated pointer, null pointer, or premature
        // refill.
        let _guard = TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");
        let mut alloc = ThreadAllocator::<DefaultBackend>::new();

        // Allocate the first 16-byte block to materialize the page and learn
        // its capacity.
        // Safety: alloc is initialized and valid.
        let first = unsafe { alloc.alloc::<StandardPolicy>(16) };
        assert!(!first.is_null(), "initial 16-byte allocation failed");

        let first_val = first as usize;
        let segment_addr = first_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (first_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
        // Safety: segment points to a valid segment containing pages.
        let max_blocks = unsafe { (*segment).pages[page_index].max_blocks };
        assert_eq!(
            max_blocks,
            mnemosyne_core::constants::PAGE_SIZE / mnemosyne_core::constants::MIN_BLOCK_SIZE,
            "16-byte page capacity should equal PAGE_SIZE / MIN_BLOCK_SIZE"
        );

        // Drain the remainder of this exact page. We stop as soon as an
        // allocation leaves the page (a refill), because the goal is to
        // prove this page's counter reaches max_blocks without wrapping.
        let mut count = 1usize;
        let mut last = first;
        while count < max_blocks {
            // Safety: alloc is valid.
            let ptr = unsafe { alloc.alloc::<StandardPolicy>(16) };
            assert!(!ptr.is_null(), "16-byte allocation {count} failed");
            let ptr_val = ptr as usize;
            let ptr_seg = ptr_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
            let ptr_page = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
            if ptr_seg != segment_addr || ptr_page != page_index {
                // Crossed into a different page before filling this one;
                // that would only happen on a wrap/early-refill defect.
                panic!(
                    "allocation {count} left the page before saturation (max_blocks={max_blocks})"
                );
            }
            assert_ne!(
                ptr, last,
                "allocator returned a duplicate pointer at {count}"
            );
            last = ptr;
            count += 1;
        }

        // The page's allocation count must now read exactly max_blocks.
        let saturated = unsafe { (*segment).pages[page_index].alloc_count };
        assert_eq!(
            saturated, max_blocks,
            "saturated alloc_count {saturated} != max_blocks {max_blocks}"
        );
        assert!(
            unsafe { (*segment).pages[page_index].free }.is_none(),
            "free list should be empty after saturating the page"
        );

        // One more allocation must succeed by refilling a fresh page rather
        // than returning a wrapped/duplicate pointer.
        // Safety: alloc is valid.
        let overflow = unsafe { alloc.alloc::<StandardPolicy>(16) };
        assert!(!overflow.is_null(), "post-saturation allocation failed");
        let overflow_val = overflow as usize;
        let overflow_seg = overflow_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
        let overflow_page = (overflow_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
        assert!(
            overflow_seg != segment_addr || overflow_page != page_index,
            "post-saturation allocation reused the full page"
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
        let ptr = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
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
        let ptr_val = ptr as usize;
        let segment_addr = ptr_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
        let max_blocks = unsafe { (*segment).pages[page_index].max_blocks };
        for _ in 0..max_blocks {
            // Safety: alloc_a is valid.
            let ptr2 = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
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
            let ptr = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
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
        let ptr_b = unsafe { alloc_b.alloc::<StandardPolicy>(64) };
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
