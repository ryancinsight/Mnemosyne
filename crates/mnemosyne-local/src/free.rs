use crate::local_alloc::page::{push_page_front, unlink_page_from_list, with_page_list_token};
use crate::per_cpu;
use crate::{poison_freed_bytes, LocalAllocatorSelector, ThreadAllocator};
use core::ptr::NonNull;
use mnemosyne_arena::{deallocate_large_or_huge, HasSegmentPool};
use mnemosyne_core::constants::{
    MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, PAGES_PER_SEGMENT, PAGE_SHIFT, PAGE_SIZE, SEGMENT_SIZE,
};
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
    if size != 0 && size <= MAX_SMALL_ALLOC_SIZE && align <= MIN_BLOCK_SIZE {
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

    let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };
    if mnemosyne_prof::is_active() {
        unsafe { record_free_profile(ptr, page, page_index) };
    }

    if !LAYOUT_PROVES_SMALL && page.block_size == 0 {
        if P::ENABLE_POISONING {
            let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            let size = unsafe { (*segment).pages[0].alloc_count };
            let size = if size > 0 {
                size
            } else {
                unsafe { (*segment).huge_mapping_suffix_from(ptr) }
            };
            unsafe { poison_freed_bytes::<P>(ptr, size) };
        }
        let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        let _released = unsafe { deallocate_large_or_huge::<B>(ptr, segment) };
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

    #[cfg(all(windows, target_arch = "x86_64"))]
    let is_owner = {
        let tid = unsafe {
            let val: u32;
            core::arch::asm!(
                "mov {0:e}, gs:[0x48]",
                out(reg) val,
                options(nostack, preserves_flags, readonly)
            );
            val
        };
        owner.matches_thread_id(tid)
    };
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    let is_owner = {
        let current_allocator = B::get_allocator_ptr_raw();
        owner.matches(current_allocator)
    };

    if is_owner {
        debug_assert!(page.alloc_count > 0, "local free observed zero alloc_count");
        let page_free = page.free;
        let page_alloc_count = page.alloc_count;
        let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
            unsafe { (*segment).keys[page_index] }
        } else {
            0
        };
        let can_free_in_place = if page_alloc_count == 1 {
            unsafe { (*segment).is_current }
        } else {
            page.list_state != 2
        };
        if can_free_in_place {
            unsafe {
                (*block).set_next::<P>(page_free, cookie);
                page.free = Some(NonNull::new_unchecked(block));
                page.alloc_count = page_alloc_count - 1;
            }
            return;
        }

        let owner_allocator = unsafe { (*segment).owner_allocator };
        if !owner_allocator.is_null() {
            let alloc = unsafe { &mut *(owner_allocator as *mut ThreadAllocator<B>) };
            if page.list_state == 2 && page_alloc_count != 1 && !alloc.is_allocating {
                do_local_free_internal::<P, B>(alloc, block, page, segment, page_index);
                return;
            }
            if !alloc.is_allocating {
                alloc.is_allocating = true;
                let became_empty =
                    do_local_free_internal::<P, B>(alloc, block, page, segment, page_index);

                if became_empty {
                    unsafe { alloc.record_defrag_operation::<P>() };
                }

                alloc.is_allocating = false;
                return;
            }
        }
    }

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
        return;
    }

    unsafe {
        page.thread_free.push::<P>(NonNull::new_unchecked(block));
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
    let was_full = page.list_state == 2;
    let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
        unsafe { (*segment).keys[page_index] }
    } else {
        0
    };
    unsafe {
        (*block).set_next::<P>(page.free, cookie);
    }
    page.free = Some(NonNull::new_unchecked(block));

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
                    unlink_page_from_list(&mut token, alloc.full_pages.get_unchecked_mut(class), branded_page);
                    push_page_front(&mut token, &mut alloc.empty_pages, branded_page, 3);
                }
            } else {
                // Case 2: Went from full to active
                unsafe {
                    unlink_page_from_list(&mut token, alloc.full_pages.get_unchecked_mut(class), branded_page);
                    push_page_front(&mut token, alloc.active_pages.get_unchecked_mut(class), branded_page, 1);
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
                    unlink_page_from_list(&mut token, alloc.active_pages.get_unchecked_mut(class), branded_page);
                    push_page_front(&mut token, &mut alloc.empty_pages, branded_page, 3);
                }
            }
        }
    });

    becomes_empty
}
