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
    if ptr.is_null() {
        return;
    }

    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;

    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

    let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };
    if mnemosyne_prof::is_active() {
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

    if page.block_size == 0 {
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
        let is_not_full = page.list_state != 2;
        if is_not_full && (page_alloc_count != 1 || unsafe { (*segment).is_current }) {
            unsafe {
                (*block).set_next::<P>(page_free, cookie);
                page.free = Some(NonNull::new_unchecked(block));
                page.decrement_alloc_count_for_segment(segment, page_index);
            }
            return;
        }

        let current_allocator = B::get_allocator_ptr_raw();
        if !current_allocator.is_null() {
            let alloc = unsafe { &mut *(current_allocator as *mut ThreadAllocator<B>) };
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

    // Trigger defrag check on remote free (cold path)
    let current_allocator = B::get_allocator_ptr_raw();
    if !current_allocator.is_null() {
        let alloc = unsafe { &mut *(current_allocator as *mut ThreadAllocator<B>) };
        unsafe { alloc.record_defrag_operation::<P>() };
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

    if was_full {
        let class = page.size_class as usize;
        if alloc.unlink_full_page(page as *mut Page, class) {
            unsafe {
                alloc.push_active_page(NonNull::new_unchecked(page as *mut Page), class);
            }
        }
    }
    if becomes_empty && !alloc.is_current_segment(segment) {
        let class = page.size_class as usize;
        let is_only_active = unsafe {
            alloc.active_pages.get_unchecked(class).is_some_and(|head| {
                core::ptr::eq(head.as_ptr(), page as *const Page) && page.next_page.is_none()
            })
        };
        if !is_only_active {
            alloc.unlink_page(page as *mut Page, class);
            unsafe {
                alloc.push_empty_page(NonNull::new_unchecked(page as *mut Page));
            }
        }
    }
    becomes_empty
}
