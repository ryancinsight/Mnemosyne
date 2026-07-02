use crate::local_alloc::page::{
    move_page_between_lists_branded, push_page_front, unlink_page_from_list, with_page_list_token,
};
use crate::per_cpu;
use crate::{LocalAllocatorSelector, ThreadAllocator, poison_freed_bytes};
use core::ptr::NonNull;
use mnemosyne_arena::{HasSegmentPool, deallocate_large_or_huge};
use mnemosyne_core::constants::PAGE_SIZE;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Block, Page, Segment, locate_segment};

/// Frees a memory block.
///
/// # Safety
///
/// The ptr must be valid and must have been returned by a previous allocation.
#[inline(always)]
pub unsafe fn thread_free<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
) {
    unsafe { thread_free_classified::<P, B, false>(ptr) }
}

/// Frees a memory block when the caller has a valid Rust `Layout`.
///
/// The layout-proven small path monomorphizes out the large/huge classifier
/// branch while retaining the raw `thread_free` fallback for large, huge, or
/// unusual-alignment allocations.
///
/// # Safety
///
/// Same contract as [`thread_free`], and `size`/`align` must come from the
/// original allocation layout.
#[inline(always)]
pub unsafe fn thread_free_layout<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // Derive the layout-proven small fast path from the same routing decision
    // `alloc` used, so the two never disagree on whether a block is small
    // (a disagreement would treat a huge allocation as small — UB). This now
    // also covers `align > MIN_BLOCK_SIZE` small allocations served by the
    // alignment-aware small path.
    if size != 0 && crate::alloc::small_path_class(size, align).is_some() {
        unsafe { thread_free_classified::<P, B, true>(ptr) };
    } else {
        unsafe { thread_free_classified::<P, B, false>(ptr) };
    }
}

#[inline(always)]
unsafe fn thread_free_classified<
    P: AllocPolicy,
    B: HasSegmentPool + LocalAllocatorSelector<B>,
    const LAYOUT_PROVES_SMALL: bool,
>(
    ptr: *mut u8,
) {
    if ptr.is_null() {
        return;
    }

    let ptr_val = ptr as usize;
    // SAFETY: `ptr` was previously returned by this allocator, satisfying
    // `locate_segment`'s contract; it recovers the live segment header and the
    // bounded page index.
    let (segment, page_index) = unsafe { locate_segment(ptr) };

    // SAFETY: `segment`/`page_index` come from `locate_segment` on an
    // allocator-owned `ptr`, so the segment header is live and the index is in
    // bounds of its `pages` array.
    let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };
    if mnemosyne_prof::is_active() {
        unsafe { record_free_profile(ptr, page, page_index) };
    }

    if !LAYOUT_PROVES_SMALL && page.block_size == 0 {
        // SAFETY: huge-allocation metadata layout. `segment` is recovered
        // from the metadata slot one pointer slot directly preceding the
        // user payload (`(ptr as *mut *mut Segment) - 1`); every huge
        // allocation writes this slot at `allocate_large_or_huge` time.
        // The `pages[0].alloc_count` / `huge_mapping_suffix_from` reads,
        // the `poison_freed_bytes` write, and the `deallocate_large_or_huge`
        // call all stay inside the originating huge mapping.
        let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        if P::ENABLE_POISONING {
            let size = unsafe { (*segment).pages[0].alloc_count };
            let size = if size > 0 {
                size
            } else {
                unsafe { (*segment).huge_mapping_suffix_from(ptr) }
            };
            unsafe { poison_freed_bytes::<P>(ptr, size) };
        }
        let _released = unsafe { deallocate_large_or_huge::<B>(ptr, segment) };
        #[cfg(feature = "dealloc-probe")]
        crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::HugeClassifier);
        return;
    }

    debug_assert_eq!(
        (ptr_val & (PAGE_SIZE - 1)) % page.block_size,
        0,
        "small free ptr must be aligned to the page's block stride"
    );

    if P::ENABLE_POISONING {
        unsafe { poison_freed_bytes::<P>(ptr, page.block_size) };
    }

    let block = ptr as *mut Block;
    let owner = unsafe { (*segment).owner };

    #[cfg(all(windows, target_arch = "x86_64", not(miri)))]
    let (is_owner, owner_allocator) = {
        let tid = mnemosyne_core::types::current_thread_id();
        if owner.matches_thread_id(tid) {
            (true, unsafe { (*segment).owner_allocator })
        } else {
            (false, core::ptr::null_mut())
        }
    };
    #[cfg(any(not(all(windows, target_arch = "x86_64")), miri))]
    let (is_owner, owner_allocator) = {
        let current_allocator = B::get_allocator_ptr_raw();
        (owner.matches(current_allocator), current_allocator)
    };

    if is_owner && !owner_allocator.is_null() {
        if page.alloc_count == 0 {
            std::process::abort();
        }
        // SAFETY: `block` is a user pointer previously returned by the
        // allocator; non-nullness is the allocator invariant. Equality
        // with `page.free` is the double-free guard.
        if Some(unsafe { NonNull::new_unchecked(block) }) == page.free {
            std::process::abort();
        }
        // SAFETY: the surrounding `is_owner && !owner_allocator.is_null()`
        // was just confirmed against `segment.owner`, so `owner_allocator`
        // is the owning allocator pointer and no concurrent accessors
        // exist for the current thread.
        let alloc = unsafe { &mut *(owner_allocator as *mut ThreadAllocator<B>) };
        let page_free = page.free;
        let page_alloc_count = page.alloc_count;
        // SAFETY: `segment`/`page_index` locate this page's parent header and its
        // key slot, satisfying `cookie_for`'s contract.
        let cookie = unsafe { (*segment).cookie_for::<P>(page_index) };

        if page.list_state != 2 {
            // Page is active
            if page_alloc_count > 1 || alloc.is_current_segment(segment) {
                // Free in-place (either remains active, or is current segment).
                // SAFETY: `block` is non-null by the alloc_count / page.free
                // corruption guards above, and `page_alloc_count == page.free`'s
                // owning count; the shared commit stays inside this owned page.
                unsafe {
                    commit_in_place_free::<P>(block, page, page_free, cookie, page_alloc_count)
                };
                #[cfg(feature = "dealloc-probe")]
                crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::InPlaceSmall);
                return;
            } else if !alloc.is_allocating {
                // Page is not the current segment and this free empties it. The
                // free-list head set, the segment-aware decrement, and the
                // active→empty page-list transition are the shared commit in
                // `do_local_free_internal`; the caller adds only the re-entrancy
                // guard and the sweep-cadence bump around it.
                alloc.is_allocating = true;
                // SAFETY: `block`/`page`/`segment`/`page_index` are the validated
                // free-path inputs (guards above) with `alloc` the owning
                // allocator — exactly `do_local_free_internal`'s contract.
                let _became_empty = unsafe {
                    do_local_free_internal::<P, B>(alloc, block, page, segment, page_index)
                };
                // SAFETY: `alloc` is the exclusively-borrowed owning allocator
                // with `is_allocating` raised, the precondition of the cold sweep.
                unsafe { alloc.record_defrag_operation::<P>() };
                alloc.is_allocating = false;
                #[cfg(feature = "dealloc-probe")]
                crate::dealloc_counters::record(
                    crate::dealloc_counters::DeallocPath::ActiveFreeLastBlock,
                );
                return;
            }
        } else if !alloc.is_allocating {
            // Page is full, transitions to active (count > 1 is guaranteed since
            // max_blocks >= 8, so it never empties directly). This is the
            // full→active branch of the shared `do_local_free_internal` commit.
            // SAFETY: as above — validated free-path inputs and the owning
            // `alloc`, satisfying `do_local_free_internal`'s contract.
            let _became_empty =
                unsafe { do_local_free_internal::<P, B>(alloc, block, page, segment, page_index) };
            #[cfg(feature = "dealloc-probe")]
            crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::FullToActive);
            return;
        }
    }

    // SAFETY: `ptr`/`page`/`block` are the function's validated
    // contract inputs from the embodiment of `thread_free`'s `// # Safety`
    // rustdoc; the `#[cold]` helper handles the cross-thread / re-entrant
    // push path.
    unsafe { thread_free_cold::<P, B>(ptr, page, block) };
}

#[cold]
#[inline(never)]
unsafe fn record_free_profile(ptr: *mut u8, page: &Page, page_index: usize) {
    let size = if page_index == 0 || page.block_size == 0 {
        // Large/huge allocation: recover the size from the shared metadata-slot
        // accessor.
        // SAFETY: `page_index == 0 || block_size == 0` identifies a large/huge
        // allocation whose metadata slot precedes `ptr`, satisfying
        // `huge_allocation_size`'s precondition.
        unsafe { crate::usable_size::huge_allocation_size(ptr) }
    } else {
        page.block_size
    };
    mnemosyne_prof::on_free(ptr, size);
}

#[cold]
#[inline(never)]
unsafe fn thread_free_cold<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
    page: &mut Page,
    block: *mut Block,
) {
    if B::ENABLE_CPU_CACHE && per_cpu::try_free_cpu::<P>(ptr, page.size_class as usize) {
        #[cfg(feature = "dealloc-probe")]
        crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::ColdOrRecursing);
        return;
    }

    unsafe {
        // SAFETY: `block` came from this allocator under the same
        // backend; non-nullness is the allocator invariant. The page-
        // local atomic free list takes ownership of the pointer.
        page.thread_free.push::<P>(NonNull::new_unchecked(block));
    }
    #[cfg(feature = "dealloc-probe")]
    crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::ColdOrRecursing);
}

/// Returns true when `page` is the single page linked in the active list rooted
/// at `active_head` (it is the head and has no successor).
///
/// This is the "do not strand the last active page in the empty list" guard
/// shared by the local-free active→empty transition and the defragmentation
/// sweep; keeping it linked as an active page lets the next allocation of this
/// class reuse it without a cold refill.
///
/// # Safety
///
/// `active_head` must be the head of an intrusive active-page list owned by the
/// calling thread, and `page` must be a live page pointer from that thread's
/// allocator, so the head dereference is a valid, unaliased read.
#[inline(always)]
pub(crate) unsafe fn is_sole_active_page(
    active_head: Option<NonNull<Page>>,
    page: *const Page,
) -> bool {
    active_head.is_some_and(|head| {
        // SAFETY: `head` is a live, owner-exclusive page pointer per the caller's
        // contract; reading `next_page` is a valid shared read.
        core::ptr::eq(head.as_ptr(), page) && unsafe { (*head.as_ptr()).next_page.is_none() }
    })
}

/// Commits an in-place block free onto a page that keeps its list membership:
/// links `block` at the front of the page-local free list and decrements the
/// live count without touching any page-list or segment-occupancy state.
///
/// This is the hot "page stays active / is the current slicing segment" arm,
/// shared by `thread_free` and the small-realloc old-block free. Because the
/// page neither empties (its count stays `>= 1`) nor changes list, the plain
/// `alloc_count` decrement is correct: `decrement_alloc_count_for_segment` would
/// touch the occupancy mask only on the `count == 0` transition, which does not
/// occur here.
///
/// # Safety
///
/// `block` must be a live, non-null block previously allocated in `page` (its
/// double-free/underflow guards must already have passed), `page_free`/`cookie`
/// must be `page`'s current free-list head and encryption cookie, and
/// `page_alloc_count` must be `page.alloc_count` (`>= 1`).
#[inline(always)]
pub(crate) unsafe fn commit_in_place_free<P: AllocPolicy>(
    block: *mut Block,
    page: &mut Page,
    page_free: Option<NonNull<Block>>,
    cookie: usize,
    page_alloc_count: usize,
) {
    // SAFETY: `block` is a live, non-null block owned by `page` per the caller's
    // contract; the free-list head mutation stays inside that page.
    unsafe {
        (*block).set_next::<P>(page_free, cookie);
        page.free = Some(NonNull::new_unchecked(block));
        page.alloc_count = page_alloc_count - 1;
    }
}

/// Internal implementation of local deallocation.
///
/// # Safety
///
/// The block pointer must point to a valid block allocated in the target page and segment.
#[inline(always)]
pub unsafe fn do_local_free_internal<P: AllocPolicy, B: HasSegmentPool>(
    alloc: &mut ThreadAllocator<B>,
    block: *mut Block,
    page: &mut Page,
    segment: *mut Segment,
    page_index: usize,
) -> bool {
    if page.alloc_count == 0 {
        std::process::abort();
    }
    // SAFETY: `block` is a user pointer the `# Safety` contract guarantees was
    // returned by a prior allocation in `page`/`segment`; non-nullness is the
    // allocator invariant, so `new_unchecked` is sound. Equality with
    // `page.free` is the double-free guard (the head was just freed).
    if Some(unsafe { NonNull::new_unchecked(block) }) == page.free {
        std::process::abort();
    }
    let was_full = page.list_state == 2;
    // SAFETY: `segment` is the live segment header owning `page` per the
    // `# Safety` contract and `page_index` is this page's index, satisfying
    // `cookie_for`'s contract.
    let cookie = unsafe { (*segment).cookie_for::<P>(page_index) };
    // SAFETY: `block` points to a valid block in `page` per the `# Safety`
    // contract; writing its embedded next pointer reinitializes the free-list
    // link and stays inside the block this caller now owns.
    unsafe {
        (*block).set_next::<P>(page.free, cookie);
    }
    // SAFETY: `block` is non-null (allocator invariant, re-confirmed by the
    // double-free guard above); publishing it as the new free-list head.
    page.free = Some(unsafe { NonNull::new_unchecked(block) });

    // SAFETY: `segment`/`page`/`page_index` are the matching segment, page, and
    // its index per the `# Safety` contract; the decrement updates this page's
    // and segment's occupancy bookkeeping under the caller's exclusive access.
    unsafe { page.decrement_alloc_count_for_segment(segment, page_index) };
    let becomes_empty = page.alloc_count == 0;

    let class = page.size_class as usize;
    let page_ptr = unsafe { NonNull::new_unchecked(page as *mut Page) };

    with_page_list_token::<B, _>(|mut token| {
        let branded_page = unsafe { token.page(page_ptr) };
        if was_full {
            if becomes_empty && !alloc.is_current_segment(segment) {
                // Case 1: Went from full directly to empty
                unsafe {
                    unlink_page_from_list(
                        &mut token,
                        alloc.full_pages.get_unchecked_mut(class),
                        branded_page,
                    );
                    push_page_front(&mut token, &mut alloc.empty_pages, branded_page, 3);
                }
            } else {
                // Case 2: Went from full to active
                unsafe {
                    move_page_between_lists_branded(
                        &mut token,
                        alloc.full_pages.get_unchecked_mut(class),
                        alloc.active_pages.get_unchecked_mut(class),
                        branded_page,
                        1,
                    );
                }
            }
        } else if becomes_empty && !alloc.is_current_segment(segment) {
            // Case 3: Went from active to empty (only if not the only active page)
            // SAFETY: `active_pages[class]` is this thread's own active-list head
            // and `page` is its live, owner-exclusive page, so the predicate's
            // head read is valid.
            let is_only_active =
                unsafe { is_sole_active_page(*alloc.active_pages.get_unchecked(class), page) };
            if !is_only_active {
                unsafe {
                    unlink_page_from_list(
                        &mut token,
                        alloc.active_pages.get_unchecked_mut(class),
                        branded_page,
                    );
                    push_page_front(&mut token, &mut alloc.empty_pages, branded_page, 3);
                }
            }
        }
    });

    becomes_empty
}
