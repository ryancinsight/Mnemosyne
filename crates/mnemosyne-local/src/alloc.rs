use crate::per_cpu;
use crate::{LocalAllocatorSelector, ThreadAllocator, initialize_allocated_bytes};
use mnemosyne_arena::{HasSegmentPool, allocate_large_or_huge};
use mnemosyne_core::constants::MIN_BLOCK_SIZE;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::size_class::{class_to_size, size_to_class_nonzero};
use mnemosyne_core::validation::{is_valid_alloc_request, is_valid_layout_alloc_request};

/// Allocates a memory block of the given size and alignment.
///
/// # Safety
///
/// This function is unsafe because it handles raw pointers and manual layouts.
///
/// The TLS allocator is keyed by `(B, P::ENABLE_FREE_LIST_ENCRYPTION)`, so
/// pages owned by standard and encrypted policies cannot share an active-page
/// list. Free and realloc operations still derive link encoding from the
/// owning segment because their caller policy may differ from the policy that
/// created the allocation.
/// Returns a null pointer rather than panicking when `size` is zero or `align`
/// is not a power of two, so callers check the result.
///
/// # Examples
///
/// ```
/// use mnemosyne_local::{thread_alloc, thread_free};
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper as Backend;
///
/// // SAFETY: 64 is nonzero, 16 is a power of two, and the block is freed
/// // exactly once below under the same policy and backend it was taken from.
/// unsafe {
///     let p = thread_alloc::<StandardPolicy, Backend>(64, 16);
///     assert!(!p.is_null());
///     assert_eq!(p as usize % 16, 0, "the returned block honours `align`");
///
///     p.write_bytes(0xA5, 64);
///     assert_eq!(*p, 0xA5);
///     assert_eq!(*p.add(63), 0xA5);
///
///     thread_free::<StandardPolicy, Backend>(p);
/// }
///
/// // An invalid request is reported, not raised.
/// // SAFETY: no allocation is produced, so nothing needs freeing.
/// unsafe {
///     assert!(thread_alloc::<StandardPolicy, Backend>(0, 16).is_null());
///     assert!(thread_alloc::<StandardPolicy, Backend>(64, 3).is_null());
/// }
/// ```
#[inline(always)]
pub unsafe fn thread_alloc<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    size: usize,
    align: usize,
) -> *mut u8 {
    if !is_valid_alloc_request(size, align) {
        return core::ptr::null_mut();
    }
    // Per-policy allocation size limit (zero means no limit beyond the global ceiling).
    if P::MAX_ALLOC_SIZE_LIMIT != 0 && size > P::MAX_ALLOC_SIZE_LIMIT {
        return core::ptr::null_mut();
    }

    let ptr = {
        // SAFETY: `is_valid_alloc_request` validated `size` and `align` above,
        // satisfying `thread_alloc_checked`'s preconditions.
        unsafe { thread_alloc_checked::<P, B>(size, align) }
    };
    if mnemosyne_prof::is_active() && !ptr.is_null() {
        mnemosyne_prof::on_alloc(ptr, size);
    }
    ptr
}

/// Allocates from a Rust `Layout`-validated request.
/// nonzero power-of-two alignment contract while still enforcing Mnemosyne's
/// allocator-specific bounds.
///
/// # Safety
///
/// `size` must be nonzero and `align` must come from a valid `Layout`.
/// # Examples
///
/// ```
/// use core::alloc::Layout;
/// use mnemosyne_local::{thread_alloc_layout, thread_free_layout};
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper as Backend;
///
/// let layout = Layout::new::<[u64; 8]>();
///
/// // SAFETY: `size`/`align` come from a `Layout`, so the alignment contract
/// // holds, and the same layout is handed back to the matching free below.
/// unsafe {
///     let p = thread_alloc_layout::<StandardPolicy, Backend>(layout.size(), layout.align());
///     assert!(!p.is_null());
///     assert_eq!(p as usize % layout.align(), 0);
///
///     p.cast::<[u64; 8]>().write([7; 8]);
///     assert_eq!(p.cast::<[u64; 8]>().read(), [7; 8]);
///
///     thread_free_layout::<StandardPolicy, Backend>(p, layout.size(), layout.align());
/// }
/// ```
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
    // SAFETY: `size` comes from a valid `Layout` (non-zero, checked above);
    // `align` is a non-zero power of two per the Layout contract.
    let ptr = unsafe { thread_alloc_checked::<P, B>(size, align) };
    if mnemosyne_prof::is_active() && !ptr.is_null() {
        mnemosyne_prof::on_alloc(ptr, size);
    }
    ptr
}

/// Size-class routing decision shared by the alloc and free paths (SSOT).
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
            // SAFETY: `adjusted_size` is non-zero and `align` is a power of two
            // — validated by `is_valid_alloc_request` upstream.
            return unsafe { allocate_large_or_huge_initialized::<P, B>(adjusted_size, align) };
        }
    };

    let slot_ptr = B::get_allocator_ptr_raw_for_policy::<P>();
    if !slot_ptr.is_null() {
        // SAFETY: `get_allocator_ptr_raw` returns this thread's TLS slot
        // address (identical in value to the allocator address, by the slot's
        // offset-0 invariant); the non-null check confirms it is initialized.
        // Gate before borrowing: reading it through a `&mut` formed here would
        // be the aliasing the gate exists to reject.
        if !unsafe { crate::tls_slot::LocalAllocatorSlot::<B>::is_allocating(slot_ptr) } {
            // SAFETY: the gate proves no outer borrow of this thread's
            // allocator is live, and the slot is thread-affine, so this `&mut`
            // is the sole live reference.
            let alloc = unsafe { &mut *(slot_ptr as *mut ThreadAllocator<B>) };
            // SAFETY: `class` is a valid size-class index from `small_path_class`
            // (bounded by `NUM_SIZE_CLASSES`), so indexing the fixed-size
            // `active_pages` array unchecked is in bounds.
            if let Some(page_ptr) = unsafe { *alloc.active_pages.get_unchecked(class) } {
                // SAFETY: `page_ptr` is a live `NonNull<Page>` taken from this
                // thread's active-page list; `alloc` holds exclusive access, so
                // no aliasing `&mut` to the page exists.
                let page = page_ptr.as_ptr();
                // SAFETY: `page` is a valid, exclusively-borrowed page of `class`;
                // the page-local fast path only touches that page's free list.
                if let Some(block) =
                    unsafe { crate::local_alloc::page::try_allocate_page_local::<P>(page) }
                {
                    let ptr = block.as_ptr() as *mut u8;
                    // SAFETY: `ptr` is a freshly carved block of at least
                    // `adjusted_size` bytes; initialization writes only within it.
                    unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                    crate::bin_stats::record_alloc_with_size(class, adjusted_size);
                    return ptr;
                }
                // SAFETY: same valid `page`; reclaim path adopts cross-thread
                // frees back into this page's local free list before allocating.
                if let Some(block) = unsafe {
                    crate::local_alloc::page::try_reclaim_and_allocate::<P>(
                        page,
                        &mut alloc.cross_thread_reclaimed,
                    )
                } {
                    let ptr = block.as_ptr() as *mut u8;
                    // SAFETY: as above, `ptr` is a fresh block of at least
                    // `adjusted_size` bytes owned by the caller.
                    unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                    crate::bin_stats::record_alloc_with_size(class, adjusted_size);
                    return ptr;
                }
            }
        }
        // SAFETY: `slot_ptr` is the live, non-null TLS slot/allocator address,
        // so `new_unchecked` produces a valid `NonNull` the cold path reuses
        // without re-reading the TLS slot. Handed over as a pointer, not a
        // borrow: the cold path re-checks the gate before forming one.
        unsafe {
            thread_alloc_cold::<P, B>(
                class,
                adjusted_size,
                align,
                Some(core::ptr::NonNull::new_unchecked(
                    slot_ptr as *mut ThreadAllocator<B>,
                )),
            )
        }
    } else {
        // SAFETY: `class`, `adjusted_size`, and `align` are all validated;
        // no TLS allocator slot is available so the cold path is used.
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
            // SAFETY: `cpu_ptr` is a freshly reserved block for `class`; the
            // initialization writes stay within that allocation.
            unsafe { initialize_allocated_bytes::<P>(cpu_ptr, adjusted_size) };
            crate::bin_stats::record_alloc_with_size(class, adjusted_size);
            return cpu_ptr;
        }
    }

    let slot_ptr = match alloc_opt {
        Some(p) => p.as_ptr().cast::<core::ffi::c_void>(),
        None => B::get_allocator_ptr_for_policy::<P>(),
    };
    if slot_ptr.is_null() {
        // SAFETY: `adjusted_size != 0` and `align` is a power of two.
        return unsafe { allocate_large_or_huge_initialized::<P, B>(adjusted_size, align) };
    }
    // SAFETY: this thread's live TLS slot address (== the allocator address).

    // Gate before borrowing.
    if unsafe { crate::tls_slot::LocalAllocatorSlot::<B>::is_allocating(slot_ptr) } {
        // SAFETY: `adjusted_size != 0` and `align` is a power of two.
        return unsafe { allocate_large_or_huge_initialized::<P, B>(adjusted_size, align) };
    }

    unsafe { crate::tls_slot::LocalAllocatorSlot::<B>::set_allocating(slot_ptr, true) };
    // SAFETY: the gate proves no other borrow of this thread's allocator is
    // live, and the slot is thread-affine.
    let alloc = unsafe { &mut *(slot_ptr as *mut ThreadAllocator<B>) };
    let ptr = unsafe { alloc.alloc_cold::<P>(class) };
    unsafe { crate::tls_slot::LocalAllocatorSlot::<B>::set_allocating(slot_ptr, false) };

    if ptr.is_null() {
        // SAFETY: `adjusted_size != 0` and `align` is a power of two.
        return unsafe { allocate_large_or_huge_initialized::<P, B>(adjusted_size, align) };
    }
    // SAFETY: `ptr` is a freshly allocated block for `class`; initialization
    // writes stay within the allocation.
    unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
    crate::bin_stats::record_alloc_with_size(class, adjusted_size);
    ptr
}

#[inline(always)]
unsafe fn allocate_large_or_huge_initialized<P: AllocPolicy, B: HasSegmentPool>(
    size: usize,
    align: usize,
) -> *mut u8 {
    let ptr = {
        // SAFETY: `allocate_large_or_huge`'s contract: `size != 0` (from the
        // caller) and `align` is a non-zero power of two (from `Layout`).
        unsafe { allocate_large_or_huge::<B>(size, align, P::ENABLE_POISONING) }
    };
    if !ptr.is_null() {
        // SAFETY: `ptr` is a freshly allocated block; `size` bytes of
        // writes stay within the allocation.
        unsafe { initialize_allocated_bytes::<P>(ptr, size) };
    }
    ptr
}
