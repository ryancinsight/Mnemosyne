//! Cold allocation path for [`super::super::ThreadAllocator`]: the slow route
//! when `alloc_class` finds no ready active-page block.
//!
//! Three stages in sequence:
//! 1. [`ThreadAllocator::alloc_cold`] — reclaim / move-full / get-new-page dispatch.
//! 2. [`ThreadAllocator::get_new_page`] — page acquisition from the empty list,
//!    current segment, or a freshly acquired segment.
//! 3. [`acquire_policy_compatible_segment`] — pop segments until one whose
//!    free-list mode matches policy `P` is found; defer incompatibles.

use crate::local_alloc::ThreadAllocator;
use core::ptr::NonNull;
use mnemosyne_arena::{HasSegmentPool, allocate_segment};
use mnemosyne_core::constants::PAGES_PER_SEGMENT;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::size_class::class_to_size;
use mnemosyne_core::types::{Page, Segment};

use super::super::page::{
    try_allocate_page_local_dynamic, try_reclaim_and_allocate_dynamic,
};

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Computes the free-list randomisation seed for a new page.
    ///
    /// Returns `rng ^ ptr_bits ^ class_hash` when `randomize` is `true`,
    /// otherwise `0` without advancing the RNG. Accepts `randomize` as a plain
    /// `bool` so callers can pass `P::RANDOMIZE_ALLOCATION` from a generic
    /// context; the compiler constant-folds the branch in both cases.
    #[inline(always)]
    fn page_init_random(&mut self, randomize: bool, ptr_bits: u64, class: usize) -> u64 {
        if randomize {
            self.next_random() ^ ptr_bits ^ (class as u64).rotate_left(17)
        } else {
            0
        }
    }

    /// Stamps metadata, builds the free list, and enqueues a page into the
    /// active list for `class` — non-generic dynamic variant.
    ///
    /// Called from `alloc_cold_raw<B>` which passes runtime bool values for
    /// `enable_encryption` and `randomize`.
    ///
    /// # Safety
    ///
    /// Same contract as `setup_and_activate_page`.
    #[inline(always)]
    unsafe fn setup_and_activate_page_dynamic(
        &mut self,
        page: *mut Page,
        segment: *mut Segment,
        page_index: usize,
        class: usize,
        block_size: usize,
        enable_encryption: bool,
        randomize: bool,
    ) {
        let random_value = self.page_init_random(randomize, page as u64, class);
        // SAFETY: `page` is a live interior page of `segment` exclusively
        // owned by this allocator; the field writes are unaliased.
        unsafe {
            (*page).block_size = block_size as _;
            (*page).size_class = class as u8;
            let page_start = Page::page_start_in_segment(segment, page_index);
            Page::initialize_free_list_in_segment_dynamic(
                segment,
                page_index,
                page_start,
                random_value,
                enable_encryption,
                randomize,
            );
            self.push_active_page(NonNull::new_unchecked(page), class);
        }
    }

}

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Cold path for allocating a block when active pages are full.
    ///
    /// Marked as `#[inline(never)]` to prevent pollution of instruction cache.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the allocator is in a valid state, the size
    /// class index is within bounds of the `active_pages` array, and the policy
    /// mode matches the mode used by all pages already owned by this allocator.
    #[inline(never)]
    pub unsafe fn alloc_cold<P: AllocPolicy>(&mut self, class: usize) -> *mut u8 {
        // SAFETY: forwarded — same contract.
        unsafe {
            self.alloc_cold_raw(class, P::ENABLE_FREE_LIST_ENCRYPTION, P::RANDOMIZE_ALLOCATION)
        }
    }

    /// Non-generic cold allocation body.
    ///
    /// `alloc_cold<P>` was `#[inline(never)]` with `3P × N backends` binary copies.
    /// Extracting the body here (parameterised by two bools instead of a full
    /// policy type) reduces it to `N` copies — one per backend.
    ///
    /// # Safety
    ///
    /// Same contract as `alloc_cold`.
    #[inline(never)]
    unsafe fn alloc_cold_raw(
        &mut self,
        class: usize,
        enable_encryption: bool,
        randomize: bool,
    ) -> *mut u8 {
        // SAFETY: this is an `unsafe fn`; the caller upholds the allocator
        // invariants, and `record_defrag_operation` only modifies bookkeeping.
        unsafe { self.record_defrag_operation(true) };
        // 1. Move the current active page to full_pages if it is indeed full.
        // SAFETY: `class` is a caller-validated size-class index.
        if let Some(active_ptr) = unsafe { *self.active_pages.get_unchecked(class) } {
            let active_page = active_ptr.as_ptr();
            if let Some(block) = unsafe {
                try_reclaim_and_allocate_dynamic(
                    active_page,
                    &mut self.cross_thread_reclaimed,
                    enable_encryption,
                    randomize,
                )
            } {
                return block.as_ptr() as *mut u8;
            }
            // Truly full — move to full_pages.
            // SAFETY: `active_ptr` is the live active page, `class` is valid.
            unsafe {
                self.unlink_page(active_ptr.as_ptr(), class);
                self.push_full_page(active_ptr, class);
            }
        }

        // 1b. Check if the new head of active_pages can satisfy the allocation.
        // SAFETY: same `class` bounds as above — valid index into `active_pages`.
        if let Some(active_ptr) = unsafe { *self.active_pages.get_unchecked(class) } {
            // SAFETY: as above; raw pointer keeps this off a `Unique` tag.
            let active_page = active_ptr.as_ptr();
            if let Some(block) = unsafe {
                try_allocate_page_local_dynamic(active_page, enable_encryption, randomize)
            } {
                return block.as_ptr() as *mut u8;
            }
            if let Some(block) = unsafe {
                try_reclaim_and_allocate_dynamic(active_page, &mut self.cross_thread_reclaimed, enable_encryption, randomize)
            } {
                return block.as_ptr() as *mut u8;
            }
        }

        // 2. Check if any page in full_pages has reclaimed cross-thread frees.
        // Limit loop to 128 pages to bound search latency under threaded saturation.
        // SAFETY: `class` is a caller-validated size-class index.
        let mut curr_opt = unsafe { *self.full_pages.get_unchecked(class) };
        let mut checked = 0;
        while let Some(page_ptr) = curr_opt {
            if checked >= 128 {
                break;
            }
            checked += 1;
            // SAFETY: `page_ptr` was walked from this allocator's own
            // `full_pages[class]` list, so the page is live and exclusively
            // owned by this thread.
            let page = page_ptr.as_ptr();
            // SAFETY: `page` is owned by this allocator. Since it is in full_pages,
            // we know it has no local free blocks. We only need to check for cross-thread frees to reclaim.
            let block_opt =
                unsafe { try_reclaim_and_allocate_dynamic(page, &mut self.cross_thread_reclaimed, enable_encryption, randomize) };
            if let Some(block) = block_opt {
                if unsafe { ((*page).alloc_count as usize) < (*page).max_blocks() } {
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
            curr_opt = unsafe { (*page).next_page };
        }

        // 3. Allocate a brand new page
        // SAFETY: `class` is the caller-validated size-class index, satisfying
        // `get_new_page`'s implicit bounds expectation; it returns either null
        // (handled below) or a page freshly installed into `active_pages`.
        let new_page_ptr = unsafe { self.get_new_page_dynamic(class, enable_encryption, randomize) };
        if new_page_ptr.is_null() {
            return core::ptr::null_mut();
        }
        self.page_refills += 1;

        // SAFETY: `new_page_ptr` is the non-null pointer just returned by
        // `get_new_page`, pointing to a freshly initialized `Page` inside a
        // segment owned exclusively by this thread, so the `&mut` is unaliased.
        let page = new_page_ptr;
        // SAFETY: `get_new_page` guarantees a freshly initialized page whose
        // free list holds at least one block.
        let block = unsafe { Page::pop_block_dynamic(page, enable_encryption, randomize) };

        // SAFETY: `page` is the freshly allocated page above with one block
        // just popped, so the count increment matches an actual allocation.
        // Addressed by segment so no page reference is minted.
        unsafe {
            let segment = Page::parent_segment_of(page);
            let page_index = (*page).index_in_segment();
            Page::increment_alloc_count_in_segment(segment, page_index);
        }

        // If it becomes full immediately, move to full list
        if unsafe { (*page).alloc_count as usize == (*page).max_blocks() } {
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

    /// Non-generic body of `get_new_page`.
    ///
    /// # Safety
    ///
    /// Same contract as `get_new_page`.
    pub(crate) unsafe fn get_new_page_dynamic(
        &mut self,
        class: usize,
        enable_encryption: bool,
        randomize: bool,
    ) -> *mut Page {
        let block_size = class_to_size(class);

        // SAFETY: `pop_best_empty_page` accesses this allocator's own empty-page
        // list under exclusive `&mut self` — no concurrent access is possible.
        if let Some(page_ptr) = unsafe { self.pop_best_empty_page() } {
            // SAFETY: `page_ptr` is a live empty page; segment/index are derived
            // from the page's own metadata, exclusively owned by this allocator.
            unsafe {
                let page = page_ptr.as_ptr();
                let segment = Page::parent_segment_of(page);
                let page_index = (*page).page_index as usize;
                self.setup_and_activate_page_dynamic(page, segment, page_index, class, block_size, enable_encryption, randomize);
                self.recycled_pages += 1;
                return page;
            }
        }

        // Prefer never-used pages in the current segment.
        if self.current_segment.is_none() || self.next_page_index >= PAGES_PER_SEGMENT {
            // SAFETY: acquires a policy-compatible segment from the OS/pools;
            // policy-incompatible orphans are returned to the orphan pool.
            if let Some(seg_ptr) = unsafe { acquire_policy_compatible_segment::<B>(enable_encryption) } {
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
                    let mut found_page_index = 0;
                    // SAFETY: `seg_ptr` is a live, mapped segment from
                    // `allocate_segment` and is now claimed exclusively by this
                    // thread; `push_owned_segment` stamps ownership before any
                    // other thread can observe it. Every `pages[i]` for
                    // `i in 1..PAGES_PER_SEGMENT` is within the segment's page
                    // array, so each raw page projection is in-bounds and each
                    // `NonNull::new_unchecked(page_ptr)`
                    // wraps a non-null interior pointer into that array.
                    unsafe {
                        self.push_owned_segment_dynamic(seg_ptr, enable_encryption);

                        self.set_current_segment(Some(NonNull::new_unchecked(seg_ptr)));
                        self.next_page_index = PAGES_PER_SEGMENT;

                        (*seg_ptr).page_linked_mask = 0;

                        for i in 1..PAGES_PER_SEGMENT {
                            let page_ptr = &raw mut (*seg_ptr).pages[i];

                            if (*page_ptr).block_size > 0 {
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
                                    enable_encryption,
                                    "adopted an orphan whose free-list mode does not match the policy"
                                );
                                let reclaimed =
                                    Page::reclaim_thread_free_if_present_for_policy(seg_ptr, i);
                                if reclaimed > 0 {
                                    self.record_cross_thread_reclaimed(reclaimed);
                                }

                                if (*page_ptr).alloc_count > 0 {
                                    let pg_class = (*page_ptr).size_class as usize;
                                    let ptr = NonNull::new_unchecked(page_ptr);
                                    if ((*page_ptr).alloc_count as usize) < (*page_ptr).max_blocks()
                                    {
                                        self.push_active_page(ptr, pg_class);
                                    } else {
                                        self.push_full_page(ptr, pg_class);
                                    }
                                } else if found_page.is_null() {
                                    found_page = page_ptr;
                                    found_page_index = i;
                                } else {
                                    self.push_empty_page(NonNull::new_unchecked(page_ptr));
                                }
                            } else if found_page.is_null() {
                                found_page = page_ptr;
                                found_page_index = i;
                            } else {
                                self.push_empty_page(NonNull::new_unchecked(page_ptr));
                            }
                        }
                    }

                    if !found_page.is_null() {
                        // SAFETY: `found_page` is a live page in `seg_ptr`'s
                        // array at `found_page_index`, exclusively owned by
                        // this allocator after the adoption above.
                        unsafe {
                            self.setup_and_activate_page_dynamic(
                                found_page, seg_ptr, found_page_index, class, block_size,
                                enable_encryption, randomize,
                            );
                        }
                        return found_page;
                    }

                    // Fallback to allocating another segment recursively.
                    // SAFETY: same caller contract — `class` is a valid
                    // size-class index and the allocator state is consistent.
                    return unsafe { self.get_new_page_dynamic(class, enable_encryption, randomize) };
                } else {
                    self.fresh_segments += 1;
                    // Fresh segment initialization
                    // SAFETY: seg_ptr is valid, exclusive to this thread, and initialized.
                    // We set owner and insert it at the head of our owned segment list.
                    unsafe {
                        self.push_owned_segment_dynamic(seg_ptr, enable_encryption);
                        self.set_current_segment(Some(NonNull::new_unchecked(seg_ptr)));
                    }
                    self.next_page_index = 1; // page 0 is segment header
                }
            } else {
                return core::ptr::null_mut();
            }
        }

        let Some(seg) = self.current_segment else {
            return core::ptr::null_mut();
        };
        let seg = seg.as_ptr();
        let page_index = self.next_page_index;
        // SAFETY: seg points to a valid Segment owned by us. We index into pages array.
        let page_ptr = unsafe { &raw mut (*seg).pages[page_index] };
        self.next_page_index += 1;

        // SAFETY: `seg` is the current live segment; `page_index` was validated
        // against `PAGES_PER_SEGMENT` by the refill condition above.
        unsafe {
            self.setup_and_activate_page_dynamic(page_ptr, seg, page_index, class, block_size, enable_encryption, randomize);
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
/// while the owner-side allocation hot paths (`pop_block`, page initialization)
/// select encryption statically from `P`; local free paths use the recorded
/// mode. A thread may therefore adopt only an orphan whose
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
///
/// `enable_encryption` must equal `P::ENABLE_FREE_LIST_ENCRYPTION` for the
/// policy `P` that will own the returned segment; it is passed as a plain
/// `bool` so this function compiles once per `B` rather than once per `(P, B)`.
#[inline(never)]
unsafe fn acquire_policy_compatible_segment<B: HasSegmentPool>(
    enable_encryption: bool,
) -> Option<*mut Segment> {
    let mut deferred: *mut Segment = core::ptr::null_mut();
    let chosen = loop {
        // SAFETY: `allocate_segment` accesses only global pool/OS state that
        // is internally synchronized; the returned segment (if any) is
        // exclusively owned by this caller.
        let Some(seg_ptr) = (unsafe { allocate_segment::<B>() }) else {
            break None;
        };
        // SAFETY: `seg_ptr` is the initialized, exclusively-owned segment just
        // returned by `allocate_segment`; `pages[1].block_size > 0`
        // distinguishes a previously-used orphan from a fresh segment, and
        // `free_list_encrypted` is its recorded chain-encoding mode.
        let incompatible_orphan = unsafe {
            (*seg_ptr).pages[1].block_size > 0
                && (*seg_ptr).free_list_encrypted != enable_encryption
        };
        if incompatible_orphan {
            // SAFETY: the segment is exclusively owned after the pop, so its
            // `next_free_segment` link is free to thread the deferral chain.
            unsafe {
                (*seg_ptr)
                    .next_free_segment
                    .store(deferred, core::sync::atomic::Ordering::Relaxed);
            }
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
            let next = (*deferred)
                .next_free_segment
                .load(core::sync::atomic::Ordering::Relaxed);
            (*deferred)
                .next_free_segment
                .store(core::ptr::null_mut(), core::sync::atomic::Ordering::Relaxed);
            B::global_orphan_pool().push_unbounded(deferred);
            deferred = next;
        }
    }
    chosen
}


