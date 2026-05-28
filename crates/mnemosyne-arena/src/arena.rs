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
/// Returns a raw pointer to the allocated block, or null on failure.
pub unsafe fn allocate_large_or_huge<B: HasSegmentPool>(size: usize, align: usize) -> *mut u8 {
    if !is_valid_alloc_request(size, align) {
        return core::ptr::null_mut();
    }
    let alignment = align;

    // Tight backend mapping size derivation.
    //
    // The mapping must satisfy `payload_end <= raw_ptr + total_alloc_size`,
    // where:
    //   aligned_addr - raw_ptr   <= SEGMENT_ALIGN - 1  (round-up to SEGMENT_ALIGN)
    //   reserved prefix          == PAGE_SIZE          (segment header window)
    //   user_addr - prefix_end   <= alignment - 1      (round-up to alignment)
    //   payload                  == size
    //
    // Summing the upper bounds yields `(SEGMENT_ALIGN - 1) + PAGE_SIZE +
    // (alignment - 1) + size`, which is strictly less than
    // `size + alignment + SEGMENT_ALIGN + PAGE_SIZE`. The prior derivation
    // reserved an entire extra `SEGMENT_SIZE` for the payload-alignment
    // slack, leaving roughly `SEGMENT_SIZE - PAGE_SIZE` (~2 MiB - 64 KiB) of
    // memory unused per huge mapping. The tighter formula removes that
    // slack while preserving the worst-case bound and the upstream
    // `MAX_ALLOC_SIZE` cap.
    let total_alloc_size = match size
        .checked_add(alignment)
        .and_then(|val| val.checked_add(SEGMENT_ALIGN))
        .and_then(|val| val.checked_add(PAGE_SIZE))
    {
        Some(val) if val <= MAX_ALLOC_SIZE => val,
        _ => return core::ptr::null_mut(),
    };

    // Safety: total_alloc_size is non-zero and safe to allocate. We call the backend allocation safely.
    let raw_ptr = unsafe { B::allocate(total_alloc_size) };
    if raw_ptr.is_null() {
        return core::ptr::null_mut();
    }

    let aligned_addr = match checked_align_up(raw_ptr as usize, SEGMENT_ALIGN) {
        Some(addr) => addr,
        None => {
            // Safety: Releasing raw memory back to the backend because alignment check overflowed.
            let _released = unsafe { B::deallocate(raw_ptr, total_alloc_size) };
            return core::ptr::null_mut();
        }
    };
    let aligned_ptr = aligned_addr as *mut Segment;

    let reserved_prefix_end = match aligned_addr.checked_add(PAGE_SIZE) {
        Some(addr) => addr,
        None => {
            // Safety: Releasing raw memory back to the backend because prefix calculation overflowed.
            let _released = unsafe { B::deallocate(raw_ptr, total_alloc_size) };
            return core::ptr::null_mut();
        }
    };

    // The user block starts after the first page, aligned to the requested alignment.
    let user_addr = match checked_align_up(reserved_prefix_end, alignment) {
        Some(addr) => addr,
        None => {
            // Safety: Releasing raw memory back to the backend because alignment check overflowed.
            let _released = unsafe { B::deallocate(raw_ptr, total_alloc_size) };
            return core::ptr::null_mut();
        }
    };
    let user_ptr = user_addr as *mut u8;

    let metadata_addr = user_addr - core::mem::size_of::<*mut Segment>();
    let payload_end = match user_addr.checked_add(size) {
        Some(addr) => addr,
        None => {
            // Safety: Releasing raw memory back to the backend because payload bound calculation overflowed.
            let _released = unsafe { B::deallocate(raw_ptr, total_alloc_size) };
            return core::ptr::null_mut();
        }
    };
    let mapping_end = match (raw_ptr as usize).checked_add(total_alloc_size) {
        Some(addr) => addr,
        None => {
            // Safety: Releasing raw memory back to the backend because mapping bound calculation overflowed.
            let _released = unsafe { B::deallocate(raw_ptr, total_alloc_size) };
            return core::ptr::null_mut();
        }
    };

    // Layout invariants enforced here:
    //
    // 1. `user_addr` is a multiple of `core::mem::align_of::<*mut Segment>()`
    //    (typically 8 on 64-bit targets). Proof: `aligned_addr` is a multiple
    //    of `SEGMENT_ALIGN >= align_of::<*mut Segment>()`, and
    //    `aligned_addr + PAGE_SIZE` is therefore also pointer-aligned. The
    //    `checked_align_up` of that base to `alignment` only adds zero or
    //    `alignment - 1` bytes; for any `alignment` that is a power of two
    //    `>= align_of::<*mut Segment>()` the result preserves pointer
    //    alignment, and for `alignment` smaller than the pointer alignment
    //    the value is already pointer-aligned and is left unchanged.
    //
    // 2. `metadata_slot = user_ptr - size_of::<*mut Segment>()` lies inside
    //    the reserved prefix before the user payload. For alignments larger
    //    than `PAGE_SIZE`, that prefix may extend beyond Page 0, so the
    //    invariant is prefix containment rather than Page-0 containment.
    //
    // 3. `alignment <= SEGMENT_SIZE`: free classification may recover the
    //    segment header by rounding the user pointer down to `SEGMENT_SIZE`
    //    unless the pointer itself is segment-aligned, in which case it uses
    //    the metadata slot directly. Rejecting larger alignments keeps that
    //    zero-copy, no-registry classification valid.
    //
    // 4. `payload_end <= mapping_end`: the mapping reserves one full segment
    //    for segment alignment slack plus `alignment` bytes for payload
    //    alignment slack, so the aligned payload and requested size remain
    //    within the raw backend mapping.
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
        Segment::initialize(aligned_ptr, raw_ptr);
        (*aligned_ptr).pages[0].block_size = total_alloc_size;

        let metadata_slot = (user_ptr as *mut *mut Segment).sub(1);
        metadata_slot.write(aligned_ptr);
    }

    user_ptr
}

/// Frees a memory block that was allocated directly from the global arena.
///
/// # Safety
///
/// The pointer must have been returned by a previous call to `allocate_large_or_huge`
/// or be a block from a segment.
#[must_use = "ignoring the release result drops the backend failure signal; bind it to `_released` when no recovery is possible"]
pub unsafe fn deallocate_large_or_huge<B: HasSegmentPool>(
    ptr: *mut u8,
    segment_ptr: *mut Segment,
) -> bool {
    // Safety: ptr is a valid large/huge allocation, so we can retrieve the segment pointer
    // from the metadata slot immediately preceding it if segment_ptr is null.
    let resolved_segment_ptr = if segment_ptr.is_null() {
        if ptr.is_null() {
            return false;
        }
        unsafe { *((ptr as *mut *mut Segment).sub(1)) }
    } else {
        segment_ptr
    };

    let segment = unsafe { &mut *resolved_segment_ptr };
    let huge_size = segment.pages[0].block_size;

    if huge_size > 0 {
        // It is a huge allocation. Release the entire OS memory mapping.
        let raw_ptr = segment.raw_alloc_ptr;
        // Safety: Releasing raw memory back to custom backend using the recorded size.
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
mod tests {
    extern crate std;

    use super::*;
    use crate::segment::GlobalSegmentPool;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use mnemosyne_core::constants::SEGMENT_SIZE;
    use mnemosyne_core::MemoryBackend;

    struct FailingHugeReleaseBackend;

    static FAILING_HUGE_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
    static FAILING_HUGE_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
    static FAILING_HUGE_DEALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    impl MemoryBackend for FailingHugeReleaseBackend {
        unsafe fn allocate(_size: usize) -> *mut u8 {
            core::ptr::null_mut()
        }

        unsafe fn deallocate(_ptr: *mut u8, _size: usize) -> bool {
            FAILING_HUGE_DEALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    impl HasSegmentPool for FailingHugeReleaseBackend {
        fn global_segment_pool() -> &'static GlobalSegmentPool {
            &FAILING_HUGE_POOL
        }

        fn global_orphan_pool() -> &'static GlobalSegmentPool {
            &FAILING_HUGE_ORPHAN_POOL
        }
    }

    #[test]
    fn huge_allocation_metadata_slot_round_trips_across_alignments() {
        let _guard = TEST_LOCK.lock().expect("arena test lock was poisoned");
        use mnemosyne_backend::MemoryBackendWrapper;
        // The fast path in `thread_alloc` already routes any `align > 16` to
        // `allocate_large_or_huge`, but the function itself must remain sound
        // for the full grid of power-of-two alignments the upstream layout
        // contract permits. We cover the entire spread from sub-pointer-size
        // alignment up to multi-page alignment.
        for &align in &[1usize, 2, 4, 8, 16, 64, 4096, 64 * 1024, 1024 * 1024] {
            let size = 4 * 1024 * 1024;
            // Safety: size is non-zero and align is a power of two.
            let user_ptr = unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(size, align) };
            assert!(!user_ptr.is_null(), "allocation failed for align {align}");
            assert_eq!(
                (user_ptr as usize) % align,
                0,
                "user pointer not aligned to {align}"
            );
            assert_eq!(
                (user_ptr as usize) % core::mem::align_of::<*mut Segment>(),
                0,
                "user pointer not pointer-aligned for metadata slot"
            );

            // Recover the segment via the metadata slot exactly the way the
            // free path does, and verify it points to the segment header
            // whose `raw_alloc_ptr` covers the user pointer.
            let recovered = unsafe { *((user_ptr as *mut *mut Segment).sub(1)) };
            assert!(
                !recovered.is_null(),
                "metadata slot returned a null segment pointer for align {align}"
            );
            let raw_ptr = unsafe { (*recovered).raw_alloc_ptr };
            let huge_size = unsafe { (*recovered).pages[0].block_size };
            assert!(
                raw_ptr as usize <= user_ptr as usize,
                "raw_ptr {raw_ptr:?} above user_ptr {user_ptr:?} for align {align}"
            );
            assert!(
                user_ptr as usize + size <= raw_ptr as usize + huge_size,
                "payload [{user_ptr:?}, +{size}) escapes mapping [{raw_ptr:?}, +{huge_size}) for align {align}"
            );
            let metadata_addr = (user_ptr as usize) - core::mem::size_of::<*mut Segment>();
            assert!(
                metadata_addr >= recovered as usize,
                "metadata slot {metadata_addr:#x} precedes segment header {:#x} for align {align}",
                recovered as usize
            );
            assert!(
                metadata_addr < user_ptr as usize,
                "metadata slot {metadata_addr:#x} not strictly before user_ptr {user_ptr:?} for align {align}"
            );

            // Safety: round-trip release using the resolved segment pointer.
            let released =
                unsafe { deallocate_large_or_huge::<MemoryBackendWrapper>(user_ptr, recovered) };
            assert!(released, "huge release failed for align {align}");
        }
    }

    #[test]
    fn huge_allocation_rejects_non_power_of_two_alignment() {
        let _guard = TEST_LOCK.lock().expect("arena test lock was poisoned");
        use mnemosyne_backend::MemoryBackendWrapper;

        for &align in &[0usize, 3, 6, 12, 24, 48, 96] {
            // Safety: this verifies local validation rejects invalid alignments
            // before any backend allocation can be observed by callers.
            let user_ptr = unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(4096, align) };
            assert!(
                user_ptr.is_null(),
                "invalid alignment {align} should be rejected"
            );
        }
    }

    #[test]
    fn huge_allocation_consumes_tight_mapping_size() {
        let _guard = TEST_LOCK.lock().expect("arena test lock was poisoned");
        use mnemosyne_backend::{backend_memory_stats, MemoryBackendWrapper};
        // The tight mapping formula reserves exactly
        // `size + alignment + SEGMENT_ALIGN + PAGE_SIZE`. Verify the backend
        // counter observed precisely that increment, so future regressions
        // that re-introduce the SEGMENT_SIZE-of-slack would fail loudly.
        let size = 4 * 1024 * 1024;
        let align = 8;
        let expected = size + align + SEGMENT_ALIGN + PAGE_SIZE;

        let before = backend_memory_stats();
        // Safety: power-of-two alignment, non-zero size.
        let user_ptr = unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(size, align) };
        assert!(
            !user_ptr.is_null(),
            "tight-mapping huge allocation returned null"
        );
        let during = backend_memory_stats();

        let mapped = during.current_mapped_bytes - before.current_mapped_bytes;
        assert_eq!(
            mapped, expected,
            "huge allocation slack regressed: mapped {mapped} bytes vs expected {expected}"
        );

        // Round-trip release; the safety contract is preserved.
        let recovered = unsafe { *((user_ptr as *mut *mut Segment).sub(1)) };
        let released =
            unsafe { deallocate_large_or_huge::<MemoryBackendWrapper>(user_ptr, recovered) };
        assert!(
            released,
            "tight-mapping round-trip release reported failure"
        );
    }

    #[test]
    fn huge_allocation_rejects_alignment_above_segment_size() {
        let _guard = TEST_LOCK.lock().expect("arena test lock was poisoned");
        use mnemosyne_backend::MemoryBackendWrapper;

        // Safety: this verifies local validation rejects alignments that would
        // break segment-rounding free classification.
        let user_ptr =
            unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(4096, SEGMENT_SIZE * 2) };
        assert!(
            user_ptr.is_null(),
            "above-segment alignment was accepted: {user_ptr:?}"
        );
    }

    #[test]
    fn huge_allocation_rejects_zero_size() {
        let _guard = TEST_LOCK.lock().expect("arena test lock was poisoned");
        use mnemosyne_backend::MemoryBackendWrapper;

        // Safety: this verifies local validation rejects zero-size direct
        // arena requests before backend allocation.
        let user_ptr = unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(0, 8) };
        assert!(
            user_ptr.is_null(),
            "zero-size huge allocation returned {user_ptr:?}"
        );
    }

    #[test]
    fn huge_allocation_rejects_request_exceeding_layout_bound() {
        let _guard = TEST_LOCK.lock().expect("arena test lock was poisoned");
        use mnemosyne_backend::MemoryBackendWrapper;

        // Safety: this verifies local validation rejects payloads whose
        // required mapping would exceed the pointer-offset-safe allocation
        // bound before any backend allocation attempt.
        let user_ptr = unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(MAX_ALLOC_SIZE, 8) };
        assert!(
            user_ptr.is_null(),
            "MAX_ALLOC_SIZE huge allocation reached backend and returned {user_ptr:?}"
        );
    }

    #[test]
    fn huge_deallocation_returns_backend_release_status() {
        let _guard = TEST_LOCK.lock().expect("arena test lock was poisoned");
        let mut segment = core::mem::MaybeUninit::<Segment>::uninit();
        let segment_ptr = segment.as_mut_ptr();

        unsafe {
            Segment::initialize(segment_ptr, 0x1000 as *mut u8);
            (*segment_ptr).pages[0].block_size = SEGMENT_SIZE * 3;
        }

        let released = unsafe {
            deallocate_large_or_huge::<FailingHugeReleaseBackend>(0x2000 as *mut u8, segment_ptr)
        };

        assert!(!released, "failing huge release backend reported success");
        assert_eq!(FAILING_HUGE_DEALLOC_CALLS.load(Ordering::Relaxed), 1);
    }
}
