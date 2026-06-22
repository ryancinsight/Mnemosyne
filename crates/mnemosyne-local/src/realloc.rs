use crate::usable_size;
use crate::{
    do_local_free_internal, initialize_allocated_bytes, poison_freed_bytes, thread_alloc_layout,
    thread_free, LocalAllocatorSelector, ThreadAllocator,
};
use core::alloc::Layout;
use core::ptr::NonNull;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::constants::{
    MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, PAGES_PER_SEGMENT, PAGE_SHIFT, SEGMENT_SIZE,
};
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::size_class::size_to_class_nonzero;
use mnemosyne_core::types::{Block, Segment};

#[inline(always)]
pub fn small_realloc_fits_existing_class(layout: Layout, new_size: usize) -> bool {
    if layout.align() > MIN_BLOCK_SIZE {
        return false;
    }

    let old_adjusted_size = core::cmp::max(layout.size(), layout.align());
    if old_adjusted_size <= 128 {
        new_size <= (old_adjusted_size + 15) & !15
    } else if old_adjusted_size <= 512 {
        new_size <= (old_adjusted_size + 31) & !31
    } else if old_adjusted_size <= 2048 {
        new_size <= (old_adjusted_size + 127) & !127
    } else if old_adjusted_size <= MAX_SMALL_ALLOC_SIZE {
        new_size <= (old_adjusted_size + 511) & !511
    } else {
        false
    }
}

/// Reallocates a memory block, optimizing performance and memory footprint by avoiding redundant
/// allocation-deallocation cycles, reusing existing size-class blocks in place, and reducing TLS
/// lookup overhead.
///
/// # Safety
///
/// Same contract as `GlobalAlloc::realloc`.
#[inline]
pub unsafe fn thread_realloc<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
    layout: Layout,
    new_size: usize,
) -> *mut u8 {
    if !ptr.is_null() && new_size != 0 {
        let is_grow = new_size > layout.size();

        let mut can_reuse = false;
        {
            let is_small =
                layout.size() <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE;

            if new_size <= layout.size() {
                if is_small {
                    if new_size >= layout.size() / 2 {
                        can_reuse = true;
                    }
                } else {
                    let current_usable = unsafe { usable_size(ptr) };
                    let new_adjusted = core::cmp::max(new_size, layout.align());
                    if new_adjusted <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                        if new_size >= layout.size() / 2 {
                            can_reuse = true;
                        }
                    } else if new_size >= layout.size() / 2 {
                        can_reuse = true;
                    } else {
                        let page_size = mnemosyne_core::constants::PAGE_SIZE;
                        let new_page_rounded = (new_adjusted + page_size - 1) & !(page_size - 1);
                        if new_page_rounded >= current_usable {
                            can_reuse = true;
                        }
                    }
                }
            } else {
                // new_size > layout.size()
                if is_small {
                    if small_realloc_fits_existing_class(layout, new_size) {
                        can_reuse = true;
                    }
                } else {
                    let current_usable = unsafe { usable_size(ptr) };
                    if new_size <= current_usable {
                        can_reuse = true;
                    }
                }
            }
        }

        if can_reuse {
            if P::ZERO_INITIALIZE && is_grow {
                unsafe {
                    core::ptr::write_bytes(ptr.add(layout.size()), 0, new_size - layout.size());
                }
            } else if P::ENABLE_POISONING && is_grow {
                unsafe {
                    core::ptr::write_bytes(
                        ptr.add(layout.size()),
                        P::POISON_ALLOC_BYTE,
                        new_size - layout.size(),
                    );
                }
            }
            if P::ENABLE_POISONING && new_size < layout.size() {
                unsafe {
                    poison_freed_bytes::<P>(ptr.add(new_size), layout.size() - new_size);
                }
            }
            return ptr;
        }
    } else {
        if ptr.is_null() {
            if new_size == 0 {
                return core::ptr::null_mut();
            }
            return unsafe { thread_alloc_layout::<P, B>(new_size, layout.align()) };
        }
        // new_size == 0 && !ptr.is_null()
        unsafe { thread_free::<P, B>(ptr) };
        return core::ptr::null_mut();
    }

    let new_adjusted = core::cmp::max(new_size, layout.align());
    let new_class = size_to_class_nonzero(new_adjusted);

    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

    let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };
    let is_old_small = page_index > 0 && page.block_size > 0;

    let mut new_ptr = core::ptr::null_mut();
    let mut local_free_done = false;

    if is_old_small {
        if let Some(class) = new_class {
            let slot_ptr = B::get_allocator_ptr_raw();
            if !slot_ptr.is_null() {
                let alloc = unsafe { &mut *(slot_ptr as *mut ThreadAllocator<B>) };
                if !alloc.is_allocating {
                    let is_owner = unsafe { (*segment).is_owned_by(|| slot_ptr) };

                    if is_owner {
                        alloc.is_allocating = true;
                        let allocated = unsafe { alloc.alloc_class::<P>(class) };
                        new_ptr = allocated;
                        if !new_ptr.is_null() {
                            unsafe {
                                initialize_allocated_bytes::<P>(new_ptr, new_adjusted);
                                core::ptr::copy_nonoverlapping(
                                    ptr,
                                    new_ptr,
                                    core::cmp::min(layout.size(), new_size),
                                );
                                let page_ref = &mut *page;
                                if P::ENABLE_POISONING {
                                    poison_freed_bytes::<P>(ptr, page_ref.block_size);
                                }
                                let block = ptr as *mut Block;
                                let page_free = page_ref.free;
                                let page_alloc_count = page_ref.alloc_count;
                                let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
                                    (*segment).keys[page_index]
                                } else {
                                    0
                                };
                                if page_free.is_some()
                                    && (page_alloc_count != 1 || (*segment).is_current)
                                {
                                    (*block).set_next::<P>(page_free, cookie);
                                    page_ref.free = Some(NonNull::new_unchecked(block));
                                    page_ref.decrement_alloc_count_for_segment(segment, page_index);
                                } else {
                                    let _became_empty = do_local_free_internal::<P, B>(
                                        alloc, block, page_ref, segment, page_index,
                                    );
                                }
                            }
                            local_free_done = true;
                        }
                        alloc.is_allocating = false;
                    }
                }
            }
        }
    }

    if new_ptr.is_null() {
        new_ptr = unsafe { thread_alloc_layout::<P, B>(new_size, layout.align()) };
        if new_ptr.is_null() {
            return core::ptr::null_mut();
        }
    }

    if !local_free_done {
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, new_ptr, core::cmp::min(layout.size(), new_size));
            thread_free::<P, B>(ptr);
        }
    }

    new_ptr
}
