//! The public free entry points.

use super::classified::thread_free_classified;
use crate::LocalAllocatorSelector;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::policy::AllocPolicy;
/// Frees a memory block.
///
/// # Safety
///
/// The ptr must be valid and must have been returned by a previous allocation.
/// A null pointer is ignored, matching `free(NULL)`.
///
/// # Examples
///
/// ```
/// use mnemosyne_local::{thread_alloc, thread_free};
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper as Backend;
///
/// // SAFETY: `p` comes from `thread_alloc` and is freed exactly once. Freeing
/// // it twice, or freeing a pointer this allocator did not return, is
/// // undefined behaviour that the allocator aborts on when it detects it.
/// unsafe {
///     let p = thread_alloc::<StandardPolicy, Backend>(32, 8);
///     assert!(!p.is_null());
///     thread_free::<StandardPolicy, Backend>(p);
///
///     // Freeing null is a no-op, so callers need no guard of their own.
///     thread_free::<StandardPolicy, Backend>(core::ptr::null_mut());
/// }
/// ```
#[inline(always)]
pub unsafe fn thread_free<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
) {
    // SAFETY: forwarded under `thread_free`'s own contract — `ptr` came from this
    // allocator and is freed once; `false` keeps the unclassified path.
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
/// # Examples
///
/// ```
/// use core::alloc::Layout;
/// use mnemosyne_local::{thread_alloc_layout, thread_free_layout};
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper as Backend;
///
/// let layout = Layout::from_size_align(96, 16).expect("96/16 is a valid layout");
///
/// // SAFETY: the `size`/`align` passed to the free are the ones the
/// // allocation was made with; a mismatched layout would misroute the free.
/// unsafe {
///     let p = thread_alloc_layout::<StandardPolicy, Backend>(layout.size(), layout.align());
///     assert!(!p.is_null());
///     thread_free_layout::<StandardPolicy, Backend>(p, layout.size(), layout.align());
/// }
/// ```
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
    // SAFETY: `thread_free_layout`'s contract holds (`ptr` from this allocator, freed
    // once, `size`/`align` as allocated), so the small-path classification below is
    // the one `alloc` made and the chosen arm frees the block on the path that
    // produced it.
    if size != 0 && crate::alloc::small_path_class(size, align).is_some() {
        unsafe { thread_free_classified::<P, B, true>(ptr) };
    } else {
        unsafe { thread_free_classified::<P, B, false>(ptr) };
    }
}
