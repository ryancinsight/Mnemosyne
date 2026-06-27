use crate::local_alloc::page::{
    move_active_page_to_empty_branded, move_full_page_to_active_branded, push_page_front,
    unlink_page_from_list, with_page_list_token,
};
use crate::per_cpu;
use crate::{poison_freed_bytes, LocalAllocatorSelector, ThreadAllocator};
use core::ptr::NonNull;
use mnemosyne_arena::{deallocate_large_or_huge, HasSegmentPool};
use mnemosyne_core::constants::{PAGES_PER_SEGMENT, PAGE_SHIFT, PAGE_SIZE, SEGMENT_SIZE};
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Block, Page, Segment};

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
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;

    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

    // SAFETY: `ptr` was previously returned by this allocator; masking
    // `ptr_val` down by `SEGMENT_SIZE` recovers a live segment header
    // initialized at allocation time. The page index is bounded by the
    // `(PAGES_PER_SEGMENT - 1)` mask.
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
        let tid = unsafe {
            // SAFETY: `gs:[0x48]` reads TEB ClientId.UniqueThread;
            // the output is the current thread id used only for
            // owner comparison. Stack/flags preserved by asm options.
            let val: u32;
            core::arch::asm!(
                "mov {0:e}, gs:[0x48]",
                out(reg) val,
                options(nostack, preserves_flags, readonly)
            );
            val
        };
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
        if Some(NonNull::new_unchecked(block)) == page.free {
            std::process::abort();
        }
        // SAFETY: the surrounding `is_owner && !owner_allocator.is_null()`
        // was just confirmed against `segment.owner`, so `owner_allocator`
        // is the owning allocator pointer and no concurrent accessors
        // exist for the current thread.
        let alloc = unsafe { &mut *(owner_allocator as *mut ThreadAllocator<B>) };
        let page_free = page.free;
        let page_alloc_count = page.alloc_count;
        let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
            unsafe { (*segment).keys[page_index] }
        } else {
            0
        };

        if page.list_state != 2 {
            // Page is active
            if page_alloc_count > 1 || alloc.is_current_segment(segment) {
                // Free in-place (either remains active, or is current segment)
                // SAFETY: `block` is non-null by the alloc_count /
                // page.free corruption guard above; the free-list head
                // mutation stays inside the page this caller owns.
                unsafe {
                    (*block).set_next::<P>(page_free, cookie);
                    page.free = Some(NonNull::new_unchecked(block));
                    page.alloc_count = page_alloc_count - 1;
                }
                #[cfg(feature = "dealloc-probe")]
                crate::dealloc_counters::record(crate::dealloc_counters::DeallocPath::InPlaceSmall);
                return;
            } else if !alloc.is_allocating {
                // Page is not current segment, and becomes empty
                alloc.is_allocating = true;
                // SAFETY: `block` is non-null by the alloc_count /
                // page.free guard above; the free-list head set, the
                // alloc_count decrement, and the branded
                // active→empty page-list move all stay within this
                // page and the brand-guarded page-list permission
                // carried by `with_page_list_token`.
                unsafe {
                    (*block).set_next::<P>(page_free, cookie);
                    page.free = Some(NonNull::new_unchecked(block));
                    page.decrement_alloc_count_for_segment(segment, page_index);

                    let class = page.size_class as usize;
                    let page_ptr = NonNull::new_unchecked(page as *mut Page);
                    with_page_list_token::<B, _>(|mut token| {
                        let branded_page = token.page(page_ptr);
                        let is_only_active =
                            alloc.active_pages.get_unchecked(class).is_some_and(|head| {
                                core::ptr::eq(head.as_ptr(), page as *const Page)
                                    && page.next_page.is_none()
                            });
                        if !is_only_active {
                            move_active_page_to_empty_branded(
                                &mut token,
                                alloc.active_pages.get_unchecked_mut(class),
                                &mut alloc.empty_pages,
                                branded_page,
                            );
                        }
                    });

                    alloc.record_defrag_operation::<P>();
                }
                alloc.is_allocating = false;
                #[cfg(feature = "dealloc-probe")]
                crate::dealloc_counters::record(
                    crate::dealloc_counters::DeallocPath::ActiveFreeLastBlock,
                );
                return;
            }
        } else if !alloc.is_allocating {
            // Page is full, transitions to active (count > 1 is guaranteed since max_blocks >= 8)
            // SAFETY: `block` is non-null by the alloc_count /
            // page.free guard above; the free-list head and
            // alloc_count decrement stay inside this page, and the
            // branded `move_full_page_to_active_branded` carries the
            // page-list token granting exclusive access to the
            // full/active page lists.
            unsafe {
                (*block).set_next::<P>(page_free, cookie);
                page.free = Some(NonNull::new_unchecked(block));
                page.alloc_count = page_alloc_count - 1;

                let class = page.size_class as usize;
                let page_ptr = NonNull::new_unchecked(page as *mut Page);
                with_page_list_token::<B, _>(|mut token| {
                    let branded_page = token.page(page_ptr);
                    move_full_page_to_active_branded(
                        &mut token,
                        alloc.full_pages.get_unchecked_mut(class),
                        alloc.active_pages.get_unchecked_mut(class),
                        branded_page,
                    );
                });
            }
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
        let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        let size = unsafe { (*segment).pages[0].alloc_count };
        if size > 0 {
            size
        } else {
            unsafe { (*segment).huge_mapping_suffix_from(ptr) }
        }
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
    if Some(NonNull::new_unchecked(block)) == page.free {
        std::process::abort();
    }
    let was_full = page.list_state == 2;
    // SAFETY: `segment` is the live segment header owning `page` per the
    // `# Safety` contract; `page_index` indexes its `keys` array, sized
    // `PAGES_PER_SEGMENT`, and the caller passed the page's own index.
    let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
        unsafe { (*segment).keys[page_index] }
    } else {
        0
    };
    // SAFETY: `block` points to a valid block in `page` per the `# Safety`
    // contract; writing its embedded next pointer reinitializes the free-list
    // link and stays inside the block this caller now owns.
    unsafe {
        (*block).set_next::<P>(page.free, cookie);
    }
    // SAFETY: `block` is non-null (allocator invariant, re-confirmed by the
    // double-free guard above); publishing it as the new free-list head.
    page.free = Some(NonNull::new_unchecked(block));

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
                    move_full_page_to_active_branded(
                        &mut token,
                        alloc.full_pages.get_unchecked_mut(class),
                        alloc.active_pages.get_unchecked_mut(class),
                        branded_page,
                    );
                }
            }
        } else if becomes_empty && !alloc.is_current_segment(segment) {
            // Case 3: Went from active to empty (only if not the only active page)
            let is_only_active = unsafe {
                alloc.active_pages.get_unchecked(class).is_some_and(|head| {
                    core::ptr::eq(head.as_ptr(), page as *const Page) && page.next_page.is_none()
                })
            };
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
