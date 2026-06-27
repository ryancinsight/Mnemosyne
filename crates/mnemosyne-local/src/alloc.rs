use crate::per_cpu;
use crate::{initialize_allocated_bytes, LocalAllocatorSelector, ThreadAllocator};
use mnemosyne_arena::{allocate_large_or_huge, HasSegmentPool};
use mnemosyne_core::constants::MIN_BLOCK_SIZE;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::size_class::{class_to_size, size_to_class_nonzero};
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

/// Size-class routing decision shared by the alloc and free paths (SSOT).
///
/// Returns `Some(class)` when `(size, align)` is served by the small
/// thread-cache path, or `None` when it must use the large/huge path.
///
/// The small path can serve an allocation requiring `align` bytes whenever the
/// chosen class's block stride is a multiple of `align`: pages start
/// `PAGE_SIZE`-aligned and blocks are carved at `block_size` stride, so
/// `block_size % align == 0` makes every block `align`-aligned (`align` is a
/// validated power of two). Rounding the request up to a multiple of `align`
/// first lets the lookup land on such a class for most sizes; non-power-of-two
/// stride classes (48/80/96/…) return `None` and route to the huge path. This
/// keeps small high-alignment allocations — e.g. 64-byte-aligned SIMD buffers —
/// out of the ~2 MiB-per-allocation huge path, which previously caught every
/// `align > 16` request regardless of size.
///
/// `alloc` routes on this; `free` derives its `LAYOUT_PROVES_SMALL` fast path
/// from the same decision, so the two can never disagree on whether a block is
/// small (a disagreement would be undefined behavior).
#[inline(always)]
pub(crate) fn small_path_class(size: usize, align: usize) -> Option<usize> {
    let adjusted_size = core::cmp::max(size, align);
    if align <= MIN_BLOCK_SIZE {
        // Every block is at least `MIN_BLOCK_SIZE`-aligned; no stride check.
        return size_to_class_nonzero(adjusted_size);
    }
    let rounded = (adjusted_size + align - 1) & !(align - 1);
    match size_to_class_nonzero(rounded) {
        Some(c) if class_to_size(c) & (align - 1) == 0 => Some(c),
        _ => None,
    }
}

#[inline(always)]
unsafe fn thread_alloc_checked<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    size: usize,
    align: usize,
) -> *mut u8 {
    let adjusted_size = core::cmp::max(size, align);

    let class = match small_path_class(size, align) {
        Some(c) => c,
        None => {
            return unsafe { allocate_large_or_huge_initialized::<P, B>(adjusted_size, align) };
        }
    };

    let slot_ptr = B::get_allocator_ptr_raw();
    if !slot_ptr.is_null() {
        // SAFETY: `get_allocator_ptr_raw` returns this thread's TLS allocator
        // slot; the non-null check confirms it is initialized, and the slot is
        // thread-affine so this `&mut` is the sole live reference.
        let alloc = unsafe { &mut *(slot_ptr as *mut ThreadAllocator<B>) };
        if !alloc.is_allocating {
            // SAFETY: `class` is a valid size-class index from `small_path_class`
            // (bounded by `NUM_SIZE_CLASSES`), so indexing the fixed-size
            // `active_pages` array unchecked is in bounds.
            if let Some(mut page_ptr) = unsafe { *alloc.active_pages.get_unchecked(class) } {
                // SAFETY: `page_ptr` is a live `NonNull<Page>` taken from this
                // thread's active-page list; `alloc` holds exclusive access, so
                // no aliasing `&mut` to the page exists.
                let page = unsafe { page_ptr.as_mut() };
                // SAFETY: `page` is a valid, exclusively-borrowed page of `class`;
                // the page-local fast path only touches that page's free list.
                if let Some(block) =
                    unsafe { crate::local_alloc::page::try_allocate_page_local::<P>(page) }
                {
                    let ptr = block.as_ptr() as *mut u8;
                    // SAFETY: `ptr` is a freshly carved block of at least
                    // `adjusted_size` bytes; initialization writes only within it.
                    unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                    return ptr;
                }
                // SAFETY: same valid `page`; reclaim path adopts cross-thread
                // frees back into this page's local free list before allocating.
                if let Some(block) =
                    unsafe { crate::local_alloc::page::try_reclaim_and_allocate::<P>(page) }
                {
                    let ptr = block.as_ptr() as *mut u8;
                    // SAFETY: as above, `ptr` is a fresh block of at least
                    // `adjusted_size` bytes owned by the caller.
                    unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                    return ptr;
                }
            }
        }
        // SAFETY: `alloc` is the live, non-null TLS allocator borrowed above, so
        // `new_unchecked` produces a valid `NonNull` the cold path reuses
        // without re-reading the TLS slot.
        unsafe {
            thread_alloc_cold::<P, B>(
                class,
                adjusted_size,
                align,
                Some(core::ptr::NonNull::new_unchecked(alloc as *mut _)),
            )
        }
    } else {
        unsafe { thread_alloc_cold::<P, B>(class, adjusted_size, align, None) }
    }
}

#[cold]
#[inline(never)]
unsafe fn thread_alloc_cold<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    class: usize,
    adjusted_size: usize,
    align: usize,
    alloc_opt: Option<core::ptr::NonNull<ThreadAllocator<B>>>,
) -> *mut u8 {
    if B::ENABLE_CPU_CACHE {
        let cpu_ptr = per_cpu::try_alloc_cpu::<P>(class);
        if !cpu_ptr.is_null() {
            unsafe { initialize_allocated_bytes::<P>(cpu_ptr, adjusted_size) };
            return cpu_ptr;
        }
    }

    let alloc = if let Some(alloc_ptr) = alloc_opt {
        unsafe { &mut *alloc_ptr.as_ptr() }
    } else {
        let slot_ptr = B::get_allocator_ptr();
        if slot_ptr.is_null() {
            return unsafe { allocate_large_or_huge_initialized::<P, B>(adjusted_size, align) };
        }
        unsafe { &mut *(slot_ptr as *mut ThreadAllocator<B>) }
    };

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
