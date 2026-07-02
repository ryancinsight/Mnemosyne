use crate::{LocalAllocatorSelector, ThreadAllocator, ThreadAllocatorStats};
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::types::{Segment, locate_segment};

/// Returns the actual usable byte count of the allocation at `ptr`.
///
/// For small allocations this returns the size-class block size (which
/// may exceed the original allocation request because Mnemosyne rounds
/// up to the next size class). For large/huge allocations it returns
/// the distance from `ptr` to the end of the recorded payload mapping.
/// Returns `0` for a null pointer.
///
/// Mirrors `mi_usable_size` (mimalloc) and `malloc_usable_size`
/// (glibc/jemalloc): the value is the maximum number of bytes the
/// caller may dereference through `ptr` without overflowing the
/// allocation. Useful for Rust `Vec<T>` capacity-rounding and for any
/// caller that wants to know the allocator's actual reservation
/// without doing a follow-up `realloc`.
///
/// # Safety
///
/// `ptr` must either be null or be a pointer previously returned by a
/// Mnemosyne allocation entry point. Calling this with a pointer that
/// originated from a different allocator is undefined behavior; the
/// function uses the same segment-rounding classification as
/// `thread_free` and dereferences the resulting segment header.
#[inline(always)]
pub unsafe fn usable_size(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }

    // SAFETY: `ptr` is a non-null allocator-owned pointer per the `# Safety`
    // contract, satisfying `locate_segment`'s precondition.
    let (segment, page_index) = unsafe { locate_segment(ptr) };

    // Safety: for small allocations, page_index is in [1, PAGES_PER_SEGMENT)
    // and the target page records the size-class block size. If page_index is
    // 0 (segment-aligned huge allocation) or the page's block_size is 0
    // (non-segment-aligned huge allocation), we route to the metadata-slot fallback.
    let page = unsafe { (*segment).pages.get_unchecked(page_index) };
    let size = page.block_size;
    if size > 0 {
        return size;
    }

    // Large/huge allocation: recover the size from the metadata-slot segment.
    // SAFETY: `page.block_size == 0` (or `page_index == 0`) identifies a
    // large/huge allocation, which stores its segment pointer in the metadata
    // slot immediately preceding the user pointer — exactly
    // `huge_allocation_size`'s precondition.
    unsafe { huge_allocation_size(ptr) }
}

/// Returns the usable byte size of a large/huge allocation from its metadata
/// slot: the recorded `pages[0].alloc_count` when set, else the mapping suffix
/// from `ptr`.
///
/// This is the single authoritative huge-allocation size recovery, shared by
/// [`usable_size`] and the free-profiling path so the metadata-slot layout and
/// its fallback live in one place.
///
/// # Safety
///
/// `ptr` must be a non-null user pointer from a Mnemosyne *large/huge*
/// allocation, so the pointer slot immediately preceding it holds a valid
/// segment header (`(ptr as *mut *mut Segment).sub(1)`).
#[inline]
pub(crate) unsafe fn huge_allocation_size(ptr: *mut u8) -> usize {
    // SAFETY: per the contract, the slot one pointer before `ptr` holds the
    // originating segment header written at `allocate_large_or_huge` time.
    let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
    let size = unsafe { (*segment).pages[0].alloc_count };
    if size > 0 {
        size
    } else {
        // SAFETY: a zero recorded size means a non-segment-aligned huge mapping;
        // `huge_mapping_suffix_from` returns the distance to the mapping end.
        unsafe { (*segment).huge_mapping_suffix_from(ptr) }
    }
}

/// Returns a statistics snapshot for the current thread allocator.
pub fn thread_allocator_stats<B: HasSegmentPool + LocalAllocatorSelector<B>>()
-> ThreadAllocatorStats {
    B::with_allocator(|alloc| alloc.stats()).unwrap_or_else(|| ThreadAllocatorStats {
        cross_thread_reclaimed_blocks: ThreadAllocator::<B>::cross_thread_reclaimed_blocks(),
        ..ThreadAllocatorStats::default()
    })
}
