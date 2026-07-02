//! Aligned segment allocations from the OS or global pools.

use super::pool::HasSegmentPool;
use super::stats::SegmentRelease;
use super::utils::checked_align_up;
use crate::numa::current_numa_node;
use mnemosyne_core::constants::{
    MAX_RETAINED_SEGMENTS_LIMIT, PAGE_SIZE, SEGMENT_ALIGN, SEGMENT_SIZE,
};
use mnemosyne_core::types::Segment;

/// Bytes requested from the OS for each standard segment mapping.
pub const SEGMENT_MAPPING_SIZE: usize = SEGMENT_SIZE * 2;

/// Free segment mappings retained for reuse.
pub const MAX_RETAINED_SEGMENTS: usize = MAX_RETAINED_SEGMENTS_LIMIT;

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

/// Size of the guard region installed at the end of Page 0.
pub const SEGMENT_HEADER_GUARD_SIZE: usize = 4096;

const _: () = assert!(SEGMENT_HEADER_GUARD_SIZE.is_power_of_two());
const _: () = assert!(SEGMENT_HEADER_GUARD_SIZE <= PAGE_SIZE);

/// Helper to pop a segment from the global segment pool or orphan pool,
/// monomorphized per backend `B` (`#[inline(never)]` keeps this cold pool path
/// out of the hot caller).
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

/// Helper to return a segment to the global segment pool, monomorphized per
/// backend `B`.
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
    // SAFETY: by this function's contract `segment` is a valid, initialized,
    // exclusively-owned `Segment`, satisfying `try_push_retained`'s own contract.
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

/// Returns the head (`[raw_ptr, aligned_addr)`) and tail
/// (`[tail_slack_start, mapping_end)`) alignment-slack subranges of a segment
/// mapping to the OS via `B::decommit`.
///
/// Both slack regions exist only to satisfy `SEGMENT_ALIGN` rounding and never
/// hold allocator or user data. On Windows `VirtualAlloc` eagerly commits the
/// whole mapping, so decommitting drops the slack's commit charge (up to
/// ~`SEGMENT_ALIGN` ≈ 2 MiB of head slack per segment) for the mapping's
/// lifetime; on Unix the slack is lazily backed, so this is typically a no-op.
/// Best-effort: a backend without decommit support (`SUPPORTS_DECOMMIT ==
/// false`) skips entirely, and both subranges stay inside the reservation,
/// which the base `B::deallocate(raw_ptr, ..)` releases in full.
///
/// # Safety
///
/// `raw_ptr` must name the base of the live backend mapping that contains both
/// subranges, with `raw_ptr as usize <= aligned_addr` and
/// `tail_slack_start <= mapping_end`. Neither subrange may hold allocator or
/// user data, and the head bounds are page-aligned (`raw_ptr` comes from the
/// backend allocator and `aligned_addr` is a `SEGMENT_ALIGN` multiple), as
/// `decommit` requires.
#[inline]
pub(crate) unsafe fn decommit_mapping_slack<B: mnemosyne_core::MemoryBackend>(
    raw_ptr: *mut u8,
    aligned_addr: usize,
    tail_slack_start: usize,
    mapping_end: usize,
) {
    if !B::SUPPORTS_DECOMMIT {
        return;
    }
    let head_slack = aligned_addr - raw_ptr as usize;
    if head_slack > 0 {
        // SAFETY: per this function's contract, `[raw_ptr, aligned_addr)` is a
        // page-aligned, data-free subrange of the live mapping.
        let _ = unsafe { B::decommit(raw_ptr, head_slack) };
    }
    if tail_slack_start < mapping_end {
        // SAFETY: per this function's contract, `[tail_slack_start,
        // mapping_end)` is a data-free subrange of the live mapping.
        let _ = unsafe { B::decommit(tail_slack_start as *mut u8, mapping_end - tail_slack_start) };
    }
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
    // SAFETY: `raw_ptr` is the non-null `SEGMENT_MAPPING_SIZE` mapping just
    // returned by `B::allocate`, which is exclusively owned and writable —
    // exactly `initialize_allocated_segment`'s contract.
    let (aligned_ptr, aligned_addr, tail_slack_start, mapping_end) =
        match unsafe { initialize_allocated_segment(raw_ptr, numa_node) } {
            Some(val) => val,
            None => {
                // Safety: Releasing raw memory back to the backend because alignment check overflowed.
                let _released = unsafe { B::deallocate(raw_ptr, SEGMENT_MAPPING_SIZE) };
                return None;
            }
        };

    #[cfg(feature = "segment-header-guards")]
    {
        if B::SUPPORTS_MAKE_GUARD {
            // Install a header guard at the end of Page 0.
            // Underflows (backward OOB writes) from Page 1 land in this guard region
            // instead of overwriting the segment metadata at the start of Page 0.
            //
            // Safety: aligned_addr + PAGE_SIZE - SEGMENT_HEADER_GUARD_SIZE is inside the mapping
            // and Page 0 is reserved strictly for Segment metadata (ending far before the guard).
            let header_guard_addr = aligned_addr + PAGE_SIZE - SEGMENT_HEADER_GUARD_SIZE;
            let _guarded =
                unsafe { B::make_guard(header_guard_addr as *mut u8, SEGMENT_HEADER_GUARD_SIZE) };
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

    // The mapping over-reserves `SEGMENT_MAPPING_SIZE = 2 * SEGMENT_SIZE` so a
    // `SEGMENT_ALIGN`-aligned base can always be found; return the resulting
    // head and tail slack to the OS (guard pages installed above lie outside
    // both slack subranges, so ordering relative to the guards is immaterial).
    //
    // SAFETY: `[raw_ptr, aligned_addr)` precedes the header and
    // `[tail_slack_start, mapping_end)` succeeds the segment pages (and tail
    // guard, when enabled), so neither holds allocator data; both stay inside
    // the live reservation covered by the base release, and the head bounds
    // are page-aligned (`raw_ptr` from `allocate`, `aligned_addr` a
    // `SEGMENT_ALIGN` multiple).
    unsafe { decommit_mapping_slack::<B>(raw_ptr, aligned_addr, tail_slack_start, mapping_end) };

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
                    // SAFETY: the backend declined to release `segment`, so it
                    // remains a valid, initialized, exclusively-owned `Segment`;
                    // returning it to the pool keeps it live and reusable.
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
    let pool = B::global_segment_pool();
    // Detach each node's retained chain with `take_all` — a single lock-free
    // atomic swap of the tagged head — then run the OS-release syscalls on the
    // privately-owned detached chain. One swap per node instead of one CAS per
    // segment, so the decay thread never serializes round-by-round with
    // allocators pushing/popping the same head line (mirrors
    // `GlobalHugePool::purge`).
    let mut purged = 0usize;
    for node in pool.nodes() {
        let (mut head, _count) = node.take_all();
        while !head.is_null() {
            let segment = head;
            // SAFETY: `segment` is a node of the chain `take_all` atomically
            // detached from this pool, so it is a valid, exclusively-owned
            // `Segment`; `next` is read before the mapping is released.
            head = unsafe { (*segment).next_free_segment };
            match unsafe { release_segment_mapping::<B>(segment) } {
                SegmentRelease::Released => purged += 1,
                SegmentRelease::RetainedAfterFailure => {
                    // The backend declined to release `segment`; re-cache it and
                    // every still-unprocessed segment for this node, then stop
                    // sweeping it (matching the prior stop-on-failure behavior so
                    // pool metadata never claims a purge for a mapping we own).
                    unsafe { node.push_unbounded(segment) };
                    while !head.is_null() {
                        let s = head;
                        head = unsafe { (*s).next_free_segment };
                        unsafe { node.push_unbounded(s) };
                    }
                    break;
                }
            }
        }
    }
    // One purge "call" per invocation, with the total released count (preserves
    // the prior telemetry contract).
    pool.record_purge(purged);

    // Safety: Releases all cached huge blocks back to the OS.
    unsafe { B::global_huge_pool().purge::<B>() };
}

/// Drops the physical backing of every retained free segment without
/// removing them from the cache.
///
/// Detaches each node's retained chain in one lock-free `take_all` swap,
/// asks the backend to reset the physical pages of each detached
/// segment's mapping, and pushes the segments back onto the pool so
/// they remain available for reuse. The address ranges stay owned by
/// the allocator; only the OS-visible RSS is released.
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
    // Detach each node's chain in one lock-free `take_all` swap, reset each
    // segment's user pages, and re-cache it (segments stay owned by the
    // allocator; only RSS drops). Batch-detaching costs one atomic swap per
    // node instead of one CAS per segment on the drain.
    let mut reset_count = 0usize;
    for node in pool.nodes() {
        let (mut head, _count) = node.take_all();
        while !head.is_null() {
            let segment = head;
            // SAFETY: `segment` is a node of the chain `take_all` atomically
            // detached from this pool, so it is a valid, exclusively-owned
            // `Segment`.
            // `next` is read before the links are cleared. Per this function's
            // contract the segment is unused, so resetting
            // `[segment + PAGE_SIZE, segment + SEGMENT_SIZE)` — its user pages,
            // never the page-0 header — discards no live data, and pushing it
            // back keeps it cached for reuse.
            head = unsafe { (*segment).next_free_segment };
            unsafe {
                (*segment).next_free_segment = core::ptr::null_mut();
                let reset_ptr = (segment as usize + PAGE_SIZE) as *mut u8;
                let reset_size = SEGMENT_SIZE - PAGE_SIZE;
                if B::page_reset(reset_ptr, reset_size) {
                    reset_count += 1;
                }
                node.push_unbounded(segment);
            }
        }
    }
    // One reset "call" per invocation, with the total reset count.
    pool.record_reset(reset_count);
}
