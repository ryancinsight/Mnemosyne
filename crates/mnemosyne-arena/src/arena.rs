//! Global arena operations coordinating large and huge allocations.

use crate::segment::alloc::decommit_mapping_slack;
use crate::segment::{HasSegmentPool, checked_align_up, deallocate_segment};
use mnemosyne_core::constants::{MAX_ALLOC_SIZE, PAGE_SIZE, SEGMENT_ALIGN};
use mnemosyne_core::types::Segment;
use mnemosyne_core::validation::is_valid_alloc_request;

/// Derives the backend request layout for a large/huge allocation: the total
/// mapping size (`size` plus `SEGMENT_ALIGN` rounding room plus
/// `max(align, PAGE_SIZE)` prefix room) and the effective alignment.
///
/// Returns `None` when the request is invalid (`is_valid_alloc_request`) or
/// the padded total would exceed `MAX_ALLOC_SIZE` / overflow.
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

/// Performs the metadata initialization common to **both** fresh allocations and
/// cache-hit segments: computes the aligned user pointer, writes the metadata
/// slot, and returns the layout triple `(user_ptr, aligned_addr,
/// tail_slack_start, mapping_end)`.
///
/// # Safety
/// `raw_ptr` must be a live mapping of at least `total_alloc_size` bytes.
#[inline(always)]
unsafe fn init_segment_layout(
    raw_ptr: *mut u8,
    total_alloc_size: usize,
    alignment: usize,
    size: usize,
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
        "metadata slot {metadata_addr:#x} must remain inside reserved prefix"
    );
    debug_assert!(
        payload_end <= mapping_end,
        "payload end {payload_end:#x} must remain inside backend mapping end {mapping_end:#x}"
    );

    // Write the back-pointer and update alloc_count (live for both paths).
    unsafe {
        (*aligned_ptr).pages[0].alloc_count = size;
        let metadata_slot = (user_ptr as *mut *mut Segment).sub(1);
        metadata_slot.write(aligned_ptr);
    }

    let tail_slack_start = checked_align_up(payload_end, PAGE_SIZE)?;
    Some((user_ptr, aligned_addr, tail_slack_start, mapping_end))
}

/// Initialise a **fresh** large/huge segment from a new OS mapping.
///
/// Writes invariant header fields (`raw_alloc_ptr`, `numa_node`, and
/// `block_size`) that are set once and never change; then calls the shared
/// layout helper to set `alloc_count` and write the back-pointer.
///
/// # Safety
/// `raw_ptr` must be a live, freshly mapped region of at least
/// `total_alloc_size` bytes with no prior segment header.
#[inline(always)]
unsafe fn initialize_large_or_huge_segment_fresh(
    raw_ptr: *mut u8,
    total_alloc_size: usize,
    alignment: usize,
    size: usize,
) -> Option<(*mut u8, usize, usize, usize)> {
    let aligned_addr = checked_align_up(raw_ptr as usize, SEGMENT_ALIGN)?;
    let aligned_ptr = aligned_addr as *mut Segment;
    // SAFETY: fresh mapping — write invariant header fields.
    unsafe {
        let node = crate::current_numa_node();
        Segment::initialize(aligned_ptr, raw_ptr, node);
        (*aligned_ptr).pages[0].block_size = total_alloc_size;
    }
    // SAFETY: same contract as the caller's unsafe block.
    unsafe { init_segment_layout(raw_ptr, total_alloc_size, alignment, size) }
}

/// Initialise a **cached** large/huge segment reused from the huge-pool.
///
/// Invariant header fields (`raw_alloc_ptr`, `block_size`) are already live
/// from the original allocation; only `alloc_count` and the back-pointer need
/// refreshing.  Skipping the full `Segment::initialize` path removes a cluster
/// of header writes on every cache-hit allocation.
///
/// # Safety
/// `raw_ptr` must be a live region holding a valid, previously-initialized
/// `Segment` header at the SEGMENT_ALIGN-aligned base of the mapping.
#[inline(always)]
unsafe fn initialize_large_or_huge_segment_cached(
    raw_ptr: *mut u8,
    total_alloc_size: usize,
    alignment: usize,
    size: usize,
) -> Option<(*mut u8, usize, usize, usize)> {
    // SAFETY: same contract as the caller's unsafe block.
    unsafe { init_segment_layout(raw_ptr, total_alloc_size, alignment, size) }
}

#[inline(always)]
unsafe fn initialize_large_or_huge_segment(
    raw_ptr: *mut u8,
    total_alloc_size: usize,
    alignment: usize,
    size: usize,
    is_cache_hit: bool,
) -> Option<(*mut u8, usize, usize, usize)> {
    if is_cache_hit {
        unsafe {
            initialize_large_or_huge_segment_cached(raw_ptr, total_alloc_size, alignment, size)
        }
    } else {
        unsafe {
            initialize_large_or_huge_segment_fresh(raw_ptr, total_alloc_size, alignment, size)
        }
    }
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
    // SAFETY: `B: HasSegmentPool` exposes a valid global huge pool; `pop`
    // returns either `None` or a segment it exclusively transfers to this caller.
    let cached = unsafe { B::global_huge_pool().pop(total_alloc_size, numa_node) };
    let is_cache_hit = cached.is_some();

    let (raw_ptr, block_size) = match cached {
        Some(segment) => {
            // SAFETY: `segment` was just popped from the huge pool, so it points
            // to a valid, initialized, exclusively-owned `Segment` whose header
            // fields (`raw_alloc_ptr`, page-0 `block_size`) are live.
            unsafe { ((*segment).raw_alloc_ptr, (*segment).pages[0].block_size) }
        }
        None => {
            // SAFETY: `total_alloc_size <= MAX_ALLOC_SIZE` is non-zero (validated
            // by `derive_large_or_huge_layout`); `B::allocate` is the backend's
            // raw mapping primitive and the null result is handled below.
            let ptr = unsafe { B::allocate(total_alloc_size) };
            if ptr.is_null() {
                return core::ptr::null_mut();
            }
            (ptr, total_alloc_size)
        }
    };

    // SAFETY: `raw_ptr` is a live mapping of at least `block_size` bytes (either a
    // pooled segment's recorded mapping or a fresh `B::allocate`), and
    // `is_cache_hit` correctly distinguishes the two so the header is only
    // re-initialized for fresh mappings.
    let (user_ptr, aligned_addr, tail_slack_start, mapping_end) = match unsafe {
        initialize_large_or_huge_segment(raw_ptr, block_size, alignment, size, is_cache_hit)
    } {
        Some(val) => val,
        None => {
            // SAFETY: `raw_ptr`/`block_size` name the mapping just acquired above;
            // releasing it on the initialization-failure path matches the
            // allocating backend `B`.
            let _released = unsafe { B::deallocate(raw_ptr, block_size) };
            return core::ptr::null_mut();
        }
    };

    // Only decommit slack on newly allocated blocks from the OS to save syscalls
    if !is_cache_hit && decommit_slack {
        // SAFETY: `[raw_ptr, aligned_addr)` precedes the header and
        // `[tail_slack_start, mapping_end)` succeeds the user payload, so
        // neither holds allocator or user data; both stay inside the fresh
        // mapping released by `B::deallocate(raw_ptr, total_alloc_size)` on
        // the free path, and the head bounds are page-aligned (`raw_ptr` from
        // `B::allocate`, `aligned_addr` a `SEGMENT_ALIGN` multiple).
        unsafe {
            decommit_mapping_slack::<B>(raw_ptr, aligned_addr, tail_slack_start, mapping_end)
        };
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
        // SAFETY: per this function's contract, a `ptr` with a null `segment_ptr`
        // was returned by `allocate_large_or_huge`, which writes the owning
        // `Segment` pointer into the pointer-aligned metadata slot immediately
        // preceding `ptr`. Reading that slot recovers the segment; the value is
        // validated (non-null, segment-aligned) immediately below before use.
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

    // SAFETY: `resolved_segment_ptr` is non-null (checked above) and is either
    // the caller-supplied `segment_ptr` or the validated metadata-slot pointer,
    // both of which name a valid `Segment` exclusively owned by this free.
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
        // SAFETY: `resolved_segment_ptr` is a valid, initialized huge-allocation
        // segment exclusively owned here; `try_push` either takes ownership into
        // the pool (returns true) or leaves it untouched (returns false).
        if unsafe { B::global_huge_pool().try_push(resolved_segment_ptr, node) } {
            return true;
        }
        let raw_ptr = segment.raw_alloc_ptr;
        // SAFETY: the pool declined to cache this huge segment, so `raw_ptr`/
        // `huge_size` name its still-live OS mapping, released here through the
        // allocating backend `B`.
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
