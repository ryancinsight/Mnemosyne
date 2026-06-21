use crate::per_cpu;
use crate::{initialize_allocated_bytes, LocalAllocatorSelector, ThreadAllocator};
use mnemosyne_arena::{allocate_large_or_huge, HasSegmentPool};
use mnemosyne_core::constants::MIN_BLOCK_SIZE;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::size_class::size_to_class_nonzero;
use mnemosyne_core::validation::{is_valid_alloc_request, is_valid_layout_alloc_request};

/// Allocates a memory block of the given size and alignment.
///
/// # Safety
///
/// This function is unsafe because it handles raw pointers and manual layouts.
#[inline(always)]
pub unsafe fn thread_alloc<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    size: usize,
    align: usize,
) -> *mut u8 {
    if !is_valid_alloc_request(size, align) {
        return core::ptr::null_mut();
    }

    let ptr = unsafe { thread_alloc_checked::<P, B>(size, align) };
    if mnemosyne_prof::is_active() && !ptr.is_null() {
        mnemosyne_prof::on_alloc(ptr, size);
    }
    ptr
}

/// Allocates from a Rust `Layout`-validated request.
///
/// This preserves the global allocator hot path by relying on `Layout` for the
/// nonzero power-of-two alignment contract while still enforcing Mnemosyne's
/// allocator-specific bounds.
///
/// # Safety
///
/// `size` must be nonzero and `align` must come from a valid `Layout`.
#[inline(always)]
pub unsafe fn thread_alloc_layout<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    size: usize,
    align: usize,
) -> *mut u8 {
    if !is_valid_layout_alloc_request(size, align) {
        return core::ptr::null_mut();
    }

    debug_assert!(
        align != 0 && align.is_power_of_two(),
        "Layout-validated allocation received invalid alignment {align}"
    );
    let ptr = unsafe { thread_alloc_checked::<P, B>(size, align) };
    if mnemosyne_prof::is_active() && !ptr.is_null() {
        mnemosyne_prof::on_alloc(ptr, size);
    }
    ptr
}

#[inline(always)]
unsafe fn thread_alloc_checked<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    size: usize,
    align: usize,
) -> *mut u8 {
    if align > MIN_BLOCK_SIZE {
        return unsafe { allocate_large_or_huge_initialized::<P, B>(size, align) };
    }

    let adjusted_size = core::cmp::max(size, align);

    let class = match size_to_class_nonzero(adjusted_size) {
        Some(c) => c,
        None => {
            return unsafe { allocate_large_or_huge_initialized::<P, B>(adjusted_size, align) };
        }
    };

    let slot_ptr = B::get_allocator_ptr_raw();
    if !slot_ptr.is_null() {
        let alloc = unsafe { &mut *(slot_ptr as *mut ThreadAllocator<B>) };
        if !alloc.is_allocating {
            if let Some(mut page_ptr) = unsafe { *alloc.active_pages.get_unchecked(class) } {
                let page = unsafe { page_ptr.as_mut() };
                if let Some(block) =
                    unsafe { crate::local_alloc::page::try_allocate_page_local::<P>(page) }
                {
                    let ptr = block.as_ptr() as *mut u8;
                    unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                    return ptr;
                }
            }
        }
    }

    unsafe { thread_alloc_cold::<P, B>(class, adjusted_size, align) }
}

#[cold]
#[inline(never)]
unsafe fn thread_alloc_cold<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    class: usize,
    adjusted_size: usize,
    align: usize,
) -> *mut u8 {
    if B::ENABLE_CPU_CACHE {
        let cpu_ptr = per_cpu::try_alloc_cpu::<P>(class);
        if !cpu_ptr.is_null() {
            unsafe { initialize_allocated_bytes::<P>(cpu_ptr, adjusted_size) };
            return cpu_ptr;
        }
    }

    let slot_ptr = B::get_allocator_ptr();
    if slot_ptr.is_null() {
        return unsafe { allocate_large_or_huge_initialized::<P, B>(adjusted_size, align) };
    }

    let alloc = unsafe { &mut *(slot_ptr as *mut ThreadAllocator<B>) };
    if alloc.is_allocating {
        return unsafe { allocate_large_or_huge_initialized::<P, B>(adjusted_size, align) };
    }

    alloc.is_allocating = true;
    let ptr = unsafe { alloc.alloc_cold::<P>(class) };
    alloc.is_allocating = false;

    if ptr.is_null() {
        return unsafe { allocate_large_or_huge_initialized::<P, B>(adjusted_size, align) };
    }
    unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
    ptr
}

#[inline(always)]
unsafe fn allocate_large_or_huge_initialized<P: AllocPolicy, B: HasSegmentPool>(
    size: usize,
    align: usize,
) -> *mut u8 {
    let ptr = unsafe { allocate_large_or_huge::<B>(size, align, P::ENABLE_POISONING) };
    if !ptr.is_null() {
        unsafe { initialize_allocated_bytes::<P>(ptr, size) };
    }
    ptr
}
