//! Aligned segment allocations from the OS or global pools.

use super::pool::HasSegmentPool;
use super::stats::SegmentRelease;
use super::utils::checked_align_up;
use crate::numa::current_numa_node;
use mnemosyne_core::constants::{SEGMENT_ALIGN, SEGMENT_SIZE};
use mnemosyne_core::types::Segment;

/// Bytes requested from the OS for each standard segment mapping.
pub const SEGMENT_MAPPING_SIZE: usize = SEGMENT_SIZE * 2;

/// Free segment mappings retained for reuse.
pub const MAX_RETAINED_SEGMENTS: usize = 1024;

/// Size of the guard region installed in the slack after every segment.
///
/// The guard lives at `aligned_addr + SEGMENT_SIZE`, inside the
/// `SEGMENT_MAPPING_SIZE - SEGMENT_SIZE` of address-space slack the
/// arena reserves to satisfy `SEGMENT_ALIGN` rounding. Worst-case
/// available slack-after = `OS_PAGE_SIZE` (when the raw OS mapping
/// happened to be aligned to `SEGMENT_ALIGN - OS_PAGE_SIZE`), so the
/// guard size must not exceed the smallest supported OS page size. We
/// fix the value at 4 KiB, which is the system page size on every
/// supported Mnemosyne target (Linux/Windows/macOS-x86_64). On
/// platforms with a larger OS page size (macOS-arm64 at 16 KiB) the
/// underlying `mprotect`/`VirtualProtect` request will fail and the
/// guard install is silently skipped - the backend telemetry surfaces
/// the actual install count.
pub const SEGMENT_TAIL_GUARD_SIZE: usize = 4096;

const _: () = assert!(SEGMENT_TAIL_GUARD_SIZE.is_power_of_two());
const _: () = assert!(SEGMENT_TAIL_GUARD_SIZE <= SEGMENT_ALIGN);

/// Non-generic helper to pop a segment from the global segment pool or orphan pool.
///
/// # Safety
///
/// The caller must ensure that the global segment and orphan pools contain valid,
/// initialized `Segment` structures. The returned segment (if any) is owned by the caller.
#[inline(never)]
unsafe fn allocate_segment_from_pools<B: HasSegmentPool>() -> Option<*mut Segment> {
    // 1. Try to pop from the global segment pool
    if let Some(segment) = B::global_segment_pool().pop() {
        // Safety: segment points to a valid allocated Segment. We re-initialize
        // the segment to erase stale epoch metadata and reset it for new allocations.
        unsafe {
            let raw_ptr = (*segment).raw_alloc_ptr;
            let node = (*segment).numa_node;
            Segment::initialize(segment, raw_ptr, node);
        }
        return Some(segment);
    }

    // 2. Try to pop from the global orphan pool.
    // Safety: Returning popped orphaned segment as is, preserving active allocations.
    if let Some(segment) = B::global_orphan_pool().pop() {
        return Some(segment);
    }

    None
}

/// Non-generic helper to return a segment to the global segment pool.
///
/// # Safety
///
/// The `segment` pointer must point to a valid, initialized `Segment` exclusively owned
/// by the caller.
#[inline(always)]
unsafe fn try_return_to_pool<B: HasSegmentPool>(segment: *mut Segment) -> bool {
    debug_assert!(
        !segment.is_null(),
        "try_return_to_pool received null segment"
    );
    unsafe { B::global_segment_pool().try_push_retained(segment) }
}

/// Non-generic helper to initialize an allocated segment header and establish alignment bounds.
///
/// # Safety
///
/// The caller must guarantee:
/// - `raw_ptr` must point to a valid, exclusive, and page-aligned allocation of size `SEGMENT_MAPPING_SIZE`.
/// - The memory range must be writable to initialize the `Segment` structure.
#[inline(never)]
unsafe fn initialize_allocated_segment(
    raw_ptr: *mut u8,
    numa_node: u32,
) -> Option<(*mut Segment, usize, usize, usize)> {
    let aligned_addr = checked_align_up(raw_ptr as usize, SEGMENT_ALIGN)?;
    let aligned_ptr = aligned_addr as *mut Segment;

    // Safety: aligned_ptr is within the allocated region.
    unsafe {
        Segment::initialize(aligned_ptr, raw_ptr, numa_node);
    }

    let tail_slack_start = if cfg!(feature = "segment-tail-guards") {
        aligned_addr + SEGMENT_SIZE + SEGMENT_TAIL_GUARD_SIZE
    } else {
        aligned_addr + SEGMENT_SIZE
    };
    let mapping_end = raw_ptr as usize + SEGMENT_MAPPING_SIZE;

    Some((aligned_ptr, aligned_addr, tail_slack_start, mapping_end))
}

/// Allocates an aligned segment of memory, either from the pool or from the OS.
///
/// # Monomorphization and ZST Static Routing
///
/// The backend parameter `B` acts as a Zero-Sized Type (ZST) policy marker. Calls
/// to this function are fully monomorphized by the compiler into direct machine-code
/// calls for the target backend, preserving the zero-cost abstraction invariant.
///
/// # Safety
///
/// This function is unsafe because it allocates virtual memory from the OS/backend,
/// aligns and initializes a raw `Segment` pointer. The caller must guarantee:
/// - The backend `B` must be a valid implementor of `HasSegmentPool`.
/// - The returned pointer must eventually be returned to the pool via
///   `deallocate_segment` or released to the OS via `release_segment_mapping`.
#[inline]
pub unsafe fn allocate_segment<B: HasSegmentPool>() -> Option<*mut Segment> {
    // Safety: allocate_segment_from_pools retrieves a valid segment from pools if available.
    if let Some(segment) = unsafe { allocate_segment_from_pools::<B>() } {
        return Some(segment);
    }

    // 3. Fall back to OS allocation
    // We allocate twice the segment size to ensure we can find an aligned boundary.
    // Safety: SEGMENT_MAPPING_SIZE is non-zero and aligned. We call B::allocate.
    let raw_ptr = unsafe { B::allocate(SEGMENT_MAPPING_SIZE) };
    if raw_ptr.is_null() {
        return None;
    }

    let numa_node = current_numa_node();
    let (aligned_ptr, aligned_addr, tail_slack_start, mapping_end) =
        match unsafe { initialize_allocated_segment(raw_ptr, numa_node) } {
            Some(val) => val,
            None => {
                // Safety: Releasing raw memory back to the backend because alignment check overflowed.
                let _released = unsafe { B::deallocate(raw_ptr, SEGMENT_MAPPING_SIZE) };
                return None;
            }
        };

    if B::SUPPORTS_DECOMMIT {
        // Return the alignment slack preceding the segment header to the OS. The
        // mapping over-reserves `SEGMENT_MAPPING_SIZE = 2 * SEGMENT_SIZE` so a
        // `SEGMENT_ALIGN`-aligned base can always be found; the bytes in
        // `[raw_ptr, aligned_addr)` are never used by the allocator. On Windows
        // `VirtualAlloc` eagerly commits the whole mapping, so decommitting this
        // head slack drops up to ~`SEGMENT_ALIGN` (≈ 2 MiB) of commit charge per
        // segment; on Unix the slack is lazily backed, so this is typically a
        // no-op. Best-effort: a backend without `decommit` (default `false`)
        // simply skips. The slack stays inside the reservation and is released by
        // `deallocate(raw_ptr, SEGMENT_MAPPING_SIZE)`.
        //
        // `head_slack` is a multiple of the system page size because both
        // `raw_ptr` (from `allocate`) and `aligned_addr` (a `SEGMENT_ALIGN`
        // multiple) are page-aligned.
        let head_slack = aligned_addr - raw_ptr as usize;
        if head_slack > 0 {
            // Safety: `[raw_ptr, aligned_addr)` is a page-aligned subrange of the
            // live reservation holding no allocator data (it precedes the header)
            // and remains covered by the base release.
            let _ = unsafe { B::decommit(raw_ptr, head_slack) };
        }
    }

    #[cfg(feature = "segment-tail-guards")]
    {
        if B::SUPPORTS_MAKE_GUARD {
            // Install a tail guard immediately after the segment's user-page
            // region. Forward OOB writes that walk past Page 31 land in this
            // guard region instead of an unrelated mapping. The address lives
            // inside the `SEGMENT_MAPPING_SIZE - SEGMENT_SIZE` slack the arena
            // reserves to satisfy `SEGMENT_ALIGN` rounding, so it is always
            // part of the same backend allocation and is released together
            // with the segment by `B::deallocate(raw_ptr, SEGMENT_MAPPING_SIZE)`.
            // The install is best-effort: a backend without a `make_guard`
            // implementation (default `false`) or a kernel that declines the
            // request (e.g. macOS-arm64 where the OS page size exceeds 4 KiB)
            // silently skips, leaving the slack accessible. Backend telemetry
            // (`guard_install_calls`) surfaces the actual install count.
            //
            // Safety: aligned_addr + SEGMENT_SIZE is inside the raw mapping
            // because slack-after >= OS_PAGE_SIZE >= SEGMENT_TAIL_GUARD_SIZE on
            // supported targets. `make_guard` never invalidates the mapping.
            let tail_guard_addr = aligned_addr + SEGMENT_SIZE;
            let _guarded =
                unsafe { B::make_guard(tail_guard_addr as *mut u8, SEGMENT_TAIL_GUARD_SIZE) };
        }
    }

    if B::SUPPORTS_DECOMMIT && tail_slack_start < mapping_end {
        let tail_slack_size = mapping_end - tail_slack_start;
        // Safety: `[tail_slack_start, mapping_end)` is a page-aligned subrange of the
        // live reservation holding no allocator data (it succeeds the segment pages)
        // and remains covered by the base release.
        let _ = unsafe { B::decommit(tail_slack_start as *mut u8, tail_slack_size) };
    }

    Some(aligned_ptr)
}

/// Returns a segment to the global pool.
///
/// # Monomorphization and ZST Static Routing
///
/// The backend parameter `B` acts as a Zero-Sized Type (ZST) policy marker. Calls
/// to this function are fully monomorphized by the compiler into direct machine-code
/// calls for the target backend, preserving the zero-cost abstraction invariant.
///
/// # Safety
///
/// This function is unsafe because it takes a raw pointer to a `Segment`. The caller must guarantee:
/// - The `segment` pointer must point to a valid, initialized `Segment` exclusively owned by the caller.
/// - The backend `B` must match the backend that allocated the segment.
#[inline]
pub unsafe fn deallocate_segment<B: HasSegmentPool>(segment: *mut Segment) {
    if !segment.is_null() {
        // Safety: try_return_to_pool checks segment status and pushes it to global segment pool if space permits.
        if !unsafe { try_return_to_pool::<B>(segment) } {
            // Safety: segment is a valid allocated Segment. We extract raw_alloc_ptr
            // and deallocate the original OS mapping since the global pool is full.
            match unsafe { release_segment_mapping::<B>(segment) } {
                SegmentRelease::Released => {}
                SegmentRelease::RetainedAfterFailure => {
                    unsafe { B::global_segment_pool().push_unbounded(segment) };
                }
            }
        }
    }
}

/// Attempts to release one segment mapping to the backend.
///
/// # Monomorphization and ZST Static Routing
///
/// The backend parameter `B` acts as a Zero-Sized Type (ZST) policy marker. Calls
/// to this function are fully monomorphized by the compiler into direct machine-code
/// calls for the target backend, preserving the zero-cost abstraction invariant.
///
/// # Safety
///
/// This function is unsafe because it deallocates raw memory and releases the OS mapping.
/// The caller must guarantee:
/// - The `segment` pointer must be a valid, initialized `Segment` exclusively owned by the caller.
/// - The backend `B` must match the backend that allocated the segment.
#[inline]
pub unsafe fn release_segment_mapping<B: HasSegmentPool>(segment: *mut Segment) -> SegmentRelease {
    debug_assert!(
        !segment.is_null(),
        "release_segment_mapping received null segment"
    );
    // Safety: segment is a valid allocated Segment. We extract raw_alloc_ptr
    // and deallocate the original OS mapping.
    let released = unsafe {
        let raw_ptr = (*segment).raw_alloc_ptr;
        B::deallocate(raw_ptr, SEGMENT_MAPPING_SIZE)
    };

    if released {
        SegmentRelease::Released
    } else {
        SegmentRelease::RetainedAfterFailure
    }
}

/// Purges the global segment pool for the given backend.
///
/// # Safety
///
/// The caller must ensure that no threads are concurrently mutating the segment pool
/// or accessing purged segment memory.
pub unsafe fn purge_segment_pool<B: HasSegmentPool>() {
    let mut purged = 0;
    let pool = B::global_segment_pool();
    while let Some(segment) = pool.pop() {
        // Safety: segment is a valid allocated Segment popped from the global pool.
        // We retain ownership if the backend reports release failure, so pool
        // metadata never claims a purge for a still-owned mapping.
        match unsafe { release_segment_mapping::<B>(segment) } {
            SegmentRelease::Released => purged += 1,
            SegmentRelease::RetainedAfterFailure => {
                unsafe { pool.push_unbounded(segment) };
                break;
            }
        }
    }
    pool.record_purge(purged);

    // Safety: Releases all cached huge blocks back to the OS.
    unsafe { B::global_huge_pool().purge::<B>() };
}

/// Drops the physical backing of every retained free segment without
/// removing them from the cache.
///
/// Walks the retained pool by draining it into a fixed-size stack
/// buffer, asks the backend to reset the physical pages of each
/// drained segment's mapping, and pushes the segments back onto the
/// pool so they remain available for reuse. The address ranges stay
/// owned by the allocator; only the OS-visible RSS is released.
///
/// Used as a lighter-weight RSS-reduction knob than `purge_segment_pool`
/// for callers that want to keep the segment cache warm but reduce
/// resident set size on idle periods.
///
/// # Safety
///
/// This function is unsafe because it resets pages in active mappings. The caller
/// must guarantee that all segments in the pool are currently unused and valid
/// initialized mappings, and that no concurrent allocations are attempting to
/// read/write the pages of the segments while they are being reset.
pub unsafe fn reset_segment_pool<B: HasSegmentPool>() {
    if !B::SUPPORTS_PAGE_RESET {
        B::global_segment_pool().record_reset(0);
        return;
    }

    let pool = B::global_segment_pool();
    // Drain into a fixed-size stack buffer (the pool is bounded to
    // MAX_RETAINED_SEGMENTS, so this never overflows).
    let mut buffer: [*mut Segment; MAX_RETAINED_SEGMENTS] =
        [core::ptr::null_mut(); MAX_RETAINED_SEGMENTS];
    let mut drained = 0usize;
    while drained < MAX_RETAINED_SEGMENTS {
        match pool.pop() {
            Some(segment) => {
                buffer[drained] = segment;
                drained += 1;
            }
            None => break,
        }
    }

    // Reset each segment's mapping and push it back. The reset result is
    // advisory: a backend without `page_reset` support (or a kernel that
    // declines the advice) returns false, in which case we leave the
    // mapping untouched and simply re-cache the segment.
    let mut reset_count = 0usize;
    for slot in buffer.iter().take(drained) {
        let segment = *slot;
        // Safety: segment was popped from the retained pool above and is
        // an initialized mapping owned by this allocator until we push it
        // back below. We reset only the active committed segment range [segment, segment + SEGMENT_SIZE)
        // to avoid calling page_reset on the decommitted head/tail slack pages,
        // which would cause the system call to fail on Windows. The segment pointer
        // is page-aligned and SEGMENT_SIZE is a multiple of the system page size,
        // which matches the page_reset alignment invariants.
        if unsafe { B::page_reset(segment as *mut u8, SEGMENT_SIZE) } {
            reset_count += 1;
        }
        unsafe { pool.push_unbounded(segment) };
    }

    pool.record_reset(reset_count);
}
