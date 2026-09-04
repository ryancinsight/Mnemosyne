//! Returning a segment: to the pool if it is retained, to the OS if not.
//!
//! The allocation side lives beside this in `mod.rs`. The split is by
//! direction — what hands memory out, and what takes it back — because the
//! pool-lifetime operations (`purge`, `reset`) belong with the taking-back
//! and nothing in the handing-out reads them.

use super::super::pool::HasSegmentPool;
use super::super::stats::SegmentRelease;
use super::{SEGMENT_MAPPING_SIZE, try_return_to_pool};
use mnemosyne_core::constants::{PAGE_SIZE, SEGMENT_SIZE};
use mnemosyne_core::types::Segment;

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
        // SAFETY: try_return_to_pool checks segment status and pushes it to global segment pool if space permits.
        if !unsafe { try_return_to_pool::<B>(segment) } {
            // SAFETY: segment is a valid allocated Segment. We extract raw_alloc_ptr
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

/// Returns a segment to the global pool without ever waiting on another
/// thread's pool critical section.
///
/// [`deallocate_segment`] waits for the pool's lifetime lock for as long as its
/// holder needs. A destructor must not: the standard's rule is that destructors
/// do not block, and thread teardown stalling behind unrelated pool traffic is
/// exactly the failure that rule exists to prevent.
///
/// The disposal ladder is the blocking function's, with every wait bounded:
/// offer the segment to the retention cache; failing that — cap reached *or*
/// lock busy — hand the mapping back to the OS, which waits on the kernel
/// rather than on a peer thread; failing that, offer it once more. Returns
/// `false` only when all three decline, in which case the caller still owns
/// `segment` and must place it.
///
/// # Safety
///
/// As [`deallocate_segment`]: `segment` must be a valid, initialized `Segment`
/// exclusively owned by the caller, allocated by backend `B`. Ownership is
/// given up only when this returns `true`.
#[inline]
pub unsafe fn try_deallocate_segment<B: HasSegmentPool>(segment: *mut Segment) -> bool {
    if segment.is_null() {
        return true;
    }
    // SAFETY: `segment` is a valid, exclusively-owned `Segment` per this
    // function's contract, which is the pool's push contract.
    if unsafe { B::global_segment_pool().try_push_retained_without_waiting(segment) } {
        return true;
    }
    // SAFETY: the pool declined, so `segment` is still exclusively owned here
    // and `B` is the backend that allocated it; releasing the mapping is the
    // same step `deallocate_segment` takes when the retention cap is reached.
    match unsafe { release_segment_mapping::<B>(segment) } {
        SegmentRelease::Released => true,
        SegmentRelease::RetainedAfterFailure => {
            // SAFETY: the backend declined to release `segment`, so it remains a
            // valid, initialized, exclusively-owned `Segment`.
            unsafe { B::global_segment_pool().try_push_retained_without_waiting(segment) }
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
    // SAFETY: segment is a valid allocated Segment. We extract raw_alloc_ptr
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
    // Detach each node's retained chain with `take_all` — one lifetime-locked
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
            head = unsafe {
                (*segment)
                    .next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed)
            };
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
                        head = unsafe {
                            (*s).next_free_segment
                                .load(core::sync::atomic::Ordering::Relaxed)
                        };
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

    // SAFETY: Releases all cached huge blocks back to the OS.
    unsafe { B::global_huge_pool().purge::<B>() };
}

/// Like [`purge_segment_pool`] but retains up to `warm_threshold` committed
/// segments in the pool after the sweep.
///
/// Segments above the threshold are released to the OS as normal. Retained
/// segments stay committed so the next burst of allocations skips the
/// `VirtualAlloc`/`mmap` round-trip.
///
/// Pass `warm_threshold = 0` for identical behaviour to `purge_segment_pool`.
///
/// # Safety
///
/// Same contract as `purge_segment_pool`.
pub unsafe fn purge_segment_pool_with_warm<B: HasSegmentPool>(warm_threshold: usize) {
    let pool = B::global_segment_pool();
    let mut purged = 0usize;
    let mut kept = 0usize;
    for node in pool.nodes() {
        let (mut head, _count) = node.take_all();
        while !head.is_null() {
            let segment = head;
            // SAFETY: `segment` is exclusively owned by this purge sweep.
            head = unsafe {
                (*segment)
                    .next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed)
            };
            if kept < warm_threshold {
                // SAFETY: segment is exclusively owned; returning to the node
                // pool transfers ownership.
                unsafe { node.push_unbounded(segment) };
                kept += 1;
                continue;
            }
            match unsafe { release_segment_mapping::<B>(segment) } {
                SegmentRelease::Released => purged += 1,
                SegmentRelease::RetainedAfterFailure => {
                    unsafe { node.push_unbounded(segment) };
                    while !head.is_null() {
                        let s = head;
                        head = unsafe {
                            (*s).next_free_segment
                                .load(core::sync::atomic::Ordering::Relaxed)
                        };
                        unsafe { node.push_unbounded(s) };
                    }
                    break;
                }
            }
        }
    }
    pool.record_purge(purged);
    // SAFETY: Releases all cached huge blocks back to the OS.
    unsafe { B::global_huge_pool().purge::<B>() };
}

/// Drops the physical backing of every retained free segment without
/// removing them from the cache.
///
/// Detaches each node's retained chain in one lifetime-locked `take_all` swap,
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
    // Detach each node's chain in one lifetime-locked `take_all` swap, reset each
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
            head = unsafe {
                (*segment)
                    .next_free_segment
                    .load(core::sync::atomic::Ordering::Relaxed)
            };
            unsafe {
                (*segment)
                    .next_free_segment
                    .store(core::ptr::null_mut(), core::sync::atomic::Ordering::Relaxed);
                let reset_ptr = segment.cast::<u8>().add(PAGE_SIZE);
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
