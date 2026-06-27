use crate::alloc::small_path_class;
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
                    // SAFETY: `ptr` is non-null and, per the realloc `# Safety`
                    // contract, was returned by a Mnemosyne allocation, which is
                    // exactly `usable_size`'s precondition.
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
                    // SAFETY: `ptr` is the non-null allocation from the realloc
                    // `# Safety` contract, satisfying `usable_size`'s precondition.
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
    // Use the shared routing decision so the in-place small-realloc target class
    // honours the requested alignment. `size_to_class_nonzero(new_adjusted)`
    // alone could pick a class whose stride does not carry `align` (e.g. class
    // 224 for a 64-byte-aligned 200-byte request), yielding a misaligned block.
    // `None` falls through to the `thread_alloc_layout` path below, which routes
    // correctly (small or huge) for the alignment.
    let new_class = small_path_class(new_size, layout.align());

    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

    // SAFETY: `ptr` (non-null, allocator-owned per the `# Safety` contract)
    // masked down by `SEGMENT_SIZE` recovers its live segment header; the
    // `(PAGES_PER_SEGMENT - 1)` mask bounds `page_index` within `pages`.
    let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };
    let is_old_small = page_index > 0 && page.block_size > 0;

    let mut new_ptr = core::ptr::null_mut();
    let mut local_free_done = false;

    if is_old_small {
        if let Some(class) = new_class {
            let slot_ptr = B::get_allocator_ptr_raw();
            if !slot_ptr.is_null() {
                // SAFETY: `get_allocator_ptr_raw` returns this thread's TLS
                // allocator slot; the non-null check confirms initialization and
                // the slot is thread-affine, so this `&mut` is the sole reference.
                let alloc = unsafe { &mut *(slot_ptr as *mut ThreadAllocator<B>) };
                if !alloc.is_allocating {
                    // SAFETY: `segment` is the live header recovered from `ptr`;
                    // `is_owned_by` reads its owner field and compares against the
                    // current thread's allocator pointer.
                    let is_owner = unsafe { (*segment).is_owned_by(|| slot_ptr) };

                    if is_owner {
                        alloc.is_allocating = true;
                        // SAFETY: `alloc` is the exclusively-borrowed owning
                        // allocator and `is_allocating` is set to guard re-entry;
                        // `class` is a valid size class from `small_path_class`.
                        let allocated = unsafe { alloc.alloc_class::<P>(class) };
                        new_ptr = allocated;
                        if !new_ptr.is_null() {
                            unsafe {
                                // SAFETY: `new_ptr` is a fresh block of at least
                                // `new_adjusted` bytes; init writes only within it.
                                initialize_allocated_bytes::<P>(new_ptr, new_adjusted);
                                // SAFETY: `ptr` (old, valid for `layout.size()`)
                                // and `new_ptr` (fresh, distinct block) are
                                // non-overlapping; copy length is the smaller size.
                                core::ptr::copy_nonoverlapping(
                                    ptr,
                                    new_ptr,
                                    core::cmp::min(layout.size(), new_size),
                                );
                                // SAFETY: `page` is the exclusively-borrowed page
                                // owning the old block; reborrowing yields the sole
                                // live `&mut` for the free bookkeeping below.
                                let page_ref = &mut *page;
                                if P::ENABLE_POISONING {
                                    // SAFETY: `ptr` is the old block, valid for the
                                    // page's `block_size` bytes being poisoned.
                                    poison_freed_bytes::<P>(ptr, page_ref.block_size);
                                }
                                let block = ptr as *mut Block;
                                let page_free = page_ref.free;
                                let page_alloc_count = page_ref.alloc_count;
                                // SAFETY: `segment` owns `page`; `page_index` is
                                // that page's index into the `keys` array.
                                let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
                                    (*segment).keys[page_index]
                                } else {
                                    0
                                };
                                if page_ref.alloc_count == 0 {
                                    std::process::abort();
                                }
                                // SAFETY: `block` is the old user pointer, non-null
                                // by the allocator invariant; `new_unchecked` is
                                // sound and equality with `page_free` is the
                                // double-free guard.
                                if Some(NonNull::new_unchecked(block)) == page_free {
                                    std::process::abort();
                                }
                                if page_free.is_some()
                                    && (page_alloc_count != 1 || alloc.is_current_segment(segment))
                                {
                                    // SAFETY: in-place free — `block` links to the
                                    // prior `page_free` head and becomes the new
                                    // head; all writes stay inside this owned page.
                                    (*block).set_next::<P>(page_free, cookie);
                                    page_ref.free = Some(NonNull::new_unchecked(block));
                                    page_ref.alloc_count = page_alloc_count - 1;
                                } else {
                                    // SAFETY: `block` belongs to `page_ref` in
                                    // `segment` at `page_index`, and `alloc` owns
                                    // them — exactly `do_local_free_internal`'s
                                    // contract for the page-list transition path.
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
