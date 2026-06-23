//! Global arena operations coordinating large and huge allocations.

use crate::segment::{checked_align_up, deallocate_segment, HasSegmentPool};
use mnemosyne_core::constants::{MAX_ALLOC_SIZE, PAGE_SIZE, SEGMENT_ALIGN};
use mnemosyne_core::types::Segment;
use mnemosyne_core::validation::is_valid_alloc_request;

/// Allocates a block of memory of the given size and alignment.
///
/// If the size is small (<= 8KB), it should be routed through the thread-local
/// allocator instead of this global arena.
///
/// # Safety
///
/// This function is unsafe because it allocates raw virtual memory and performs
/// low-level pointer arithmetic. Callers must guarantee:
/// - `size` is non-zero.
/// - `align` is a non-zero power of two.
/// - `align <= SEGMENT_SIZE` (to ensure the small-free classifier can safely
///   recover the segment header via segment rounding or the metadata slot).
/// - The returned pointer must be deallocated using `deallocate_large_or_huge`
///   with the same memory backend `B`.
#[inline(always)]
fn derive_large_or_huge_layout(size: usize, align: usize) -> Option<(usize, usize)> {
    if !is_valid_alloc_request(size, align) {
        return None;
    }
    let extra = core::cmp::max(align, PAGE_SIZE);
    let total_alloc_size = size
        .checked_add(SEGMENT_ALIGN)
        .and_then(|val| val.checked_add(extra))?;

    if total_alloc_size <= MAX_ALLOC_SIZE {
        Some((total_alloc_size, align))
    } else {
        None
    }
}

#[inline(always)]
unsafe fn initialize_large_or_huge_segment(
    raw_ptr: *mut u8,
    total_alloc_size: usize,
    alignment: usize,
    size: usize,
    is_cache_hit: bool,
) -> Option<(*mut u8, usize, usize, usize)> {
    let aligned_addr = checked_align_up(raw_ptr as usize, SEGMENT_ALIGN)?;
    let aligned_ptr = aligned_addr as *mut Segment;

    let reserved_prefix_end = aligned_addr.checked_add(PAGE_SIZE)?;
    let user_addr = checked_align_up(reserved_prefix_end, alignment)?;
    let user_ptr = user_addr as *mut u8;

    let metadata_addr = user_addr - core::mem::size_of::<*mut Segment>();
    let payload_end = user_addr.checked_add(size)?;
    let mapping_end = (raw_ptr as usize).checked_add(total_alloc_size)?;

    debug_assert_eq!(
        user_addr % core::mem::align_of::<*mut Segment>(),
        0,
        "user pointer must be aligned to *mut Segment"
    );
    debug_assert!(
        metadata_addr >= aligned_addr && metadata_addr < user_addr,
        "metadata slot {metadata_addr:#x} must remain inside reserved prefix [{aligned_addr:#x}, {user_addr:#x})"
    );
    debug_assert!(
        payload_end <= mapping_end,
        "payload end {payload_end:#x} must remain inside backend mapping end {mapping_end:#x}"
    );

    // Safety: aligned_ptr is within the allocated region and aligned to a segment boundary.
    // We initialize the segment header fields and set Page 0's block_size to mark huge allocations.
    // We also write the segment pointer right before the user pointer in the unused Page 0 padding space.
    unsafe {
        if !is_cache_hit {
            let node = crate::current_numa_node();
            Segment::initialize(aligned_ptr, raw_ptr, node);
            (*aligned_ptr).pages[0].block_size = total_alloc_size;
        }
        (*aligned_ptr).pages[0].alloc_count = size;

        let metadata_slot = (user_ptr as *mut *mut Segment).sub(1);
        metadata_slot.write(aligned_ptr);
    }

    let tail_slack_start = checked_align_up(payload_end, PAGE_SIZE)?;
    Some((user_ptr, aligned_addr, tail_slack_start, mapping_end))
}

/// Allocates a block of memory of the given size and alignment.
///
/// If the size is small (<= 8KB), it should be routed through the thread-local
/// allocator instead of this global arena.
///
/// # Safety
///
/// This function is unsafe because it allocates raw virtual memory and performs
/// low-level pointer arithmetic. Callers must guarantee:
/// - `size` is non-zero.
/// - `align` is a non-zero power of two.
/// - `align <= SEGMENT_SIZE` (to ensure the small-free classifier can safely
///   recover the segment header via segment rounding or the metadata slot).
/// - The returned pointer must be deallocated using `deallocate_large_or_huge`
///   with the same memory backend `B`.
pub unsafe fn allocate_large_or_huge<B: HasSegmentPool>(
    size: usize,
    align: usize,
    decommit_slack: bool,
) -> *mut u8 {
    let (total_alloc_size, alignment) = match derive_large_or_huge_layout(size, align) {
        Some(val) => val,
        None => return core::ptr::null_mut(),
    };

    let numa_node = crate::numa::current_numa_node() as usize;
    let cached = unsafe { B::global_huge_pool().pop(total_alloc_size, numa_node) };
    let is_cache_hit = cached.is_some();

    let (raw_ptr, block_size) = match cached {
        Some(segment) => {
            let r = unsafe { (*segment).raw_alloc_ptr };
            let s = unsafe { (*segment).pages[0].block_size };
            (r, s)
        }
        None => {
            let ptr = unsafe { B::allocate(total_alloc_size) };
            if ptr.is_null() {
                return core::ptr::null_mut();
            }
            (ptr, total_alloc_size)
        }
    };

    let (user_ptr, aligned_addr, tail_slack_start, mapping_end) = match unsafe {
        initialize_large_or_huge_segment(raw_ptr, block_size, alignment, size, is_cache_hit)
    } {
        Some(val) => val,
        None => {
            let _released = unsafe { B::deallocate(raw_ptr, block_size) };
            return core::ptr::null_mut();
        }
    };

    // Only decommit slack on newly allocated blocks from the OS to save syscalls
    if !is_cache_hit && B::SUPPORTS_DECOMMIT && decommit_slack {
        // Return the alignment slack before the aligned header to the OS. As in
        // `allocate_segment`, `[raw_ptr, aligned_addr)` is never touched; on Windows
        // it is eagerly committed and would otherwise hold commit charge for the
        // lifetime of the huge allocation. Best-effort and page-aligned (both bounds
        // are page-aligned); the slack stays inside the reservation and is released
        // by the `B::deallocate(raw_ptr, total_alloc_size)` on the free path.
        let head_slack = aligned_addr - raw_ptr as usize;
        if head_slack > 0 {
            // Safety: `[raw_ptr, aligned_addr)` is a page-aligned subrange of the
            // live mapping holding no allocation data (it precedes the header).
            let _ = unsafe { B::decommit(raw_ptr, head_slack) };
        }

        if tail_slack_start < mapping_end {
            let tail_slack_size = mapping_end - tail_slack_start;
            // Safety: `[tail_slack_start, mapping_end)` is a page-aligned subrange of the
            // live reservation holding no allocator or user data (it succeeds the user payload)
            // and remains covered by the base release.
            let _ = unsafe { B::decommit(tail_slack_start as *mut u8, tail_slack_size) };
        }
    }

    user_ptr
}

/// Frees a memory block that was allocated directly from the global arena.
///
/// # Safety
///
/// This function is unsafe because it performs raw pointer dereferencing and
/// releases OS-level memory mappings. Callers must guarantee:
/// - `ptr` must be a pointer returned by a previous call to `allocate_large_or_huge`
///   or be a block from a valid segment.
/// - If `segment_ptr` is null, `ptr` must be preceded by a valid pointer-aligned
///   metadata slot containing the pointer to the owning `Segment`.
/// - If `segment_ptr` is non-null, it must point to the valid `Segment` that owns `ptr`.
/// - The backend `B` must match the backend used to allocate the block.
#[must_use = "ignoring the release result drops the backend failure signal; bind it to `_released` when no recovery is possible"]
pub unsafe fn deallocate_large_or_huge<B: HasSegmentPool>(
    ptr: *mut u8,
    segment_ptr: *mut Segment,
) -> bool {
    let resolved_segment_ptr = if segment_ptr.is_null() {
        if ptr.is_null() {
            return false;
        }
        let s = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        if s.is_null() || (s as usize) & (SEGMENT_ALIGN - 1) != 0 {
            #[cfg(any(feature = "std", test))]
            {
                std::process::abort();
            }
            #[cfg(not(any(feature = "std", test)))]
            {
                panic!("Corrupt segment pointer detected in metadata slot");
            }
        }
        s
    } else {
        segment_ptr
    };

    if resolved_segment_ptr.is_null() {
        return false;
    }

    let segment = unsafe { &mut *resolved_segment_ptr };
    let raw_ptr = segment.raw_alloc_ptr;
    let aligned_addr = resolved_segment_ptr as usize;

    if raw_ptr.is_null()
        || aligned_addr < raw_ptr as usize
        || aligned_addr - (raw_ptr as usize) >= SEGMENT_ALIGN
    {
        #[cfg(any(feature = "std", test))]
        {
            std::process::abort();
        }
        #[cfg(not(any(feature = "std", test)))]
        {
            panic!("Corrupt segment header invariants detected");
        }
    }

    let huge_size = segment.pages[0].block_size;

    if huge_size > 0 {
        // It is a huge allocation. Try to cache it first.
        let node = segment.numa_node as usize;
        if unsafe { B::global_huge_pool().try_push(resolved_segment_ptr, node) } {
            return true;
        }
        // Safety: Releasing raw memory back to custom backend using the recorded size.
        let raw_ptr = segment.raw_alloc_ptr;
        unsafe { B::deallocate(raw_ptr, huge_size) }
    } else {
        // It is a standard segment containing page allocations.
        // Return it to the global segment pool.
        // Safety: deallocate_segment is safe as resolved_segment_ptr is valid and owned by us.
        unsafe { deallocate_segment::<B>(resolved_segment_ptr) };
        true
    }
}

#[cfg(test)]
mod tests;
