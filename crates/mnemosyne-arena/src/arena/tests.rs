extern crate std;

use super::*;
use crate::segment::pool::BackendPools;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::constants::SEGMENT_SIZE;
use mnemosyne_core::MemoryBackend;

struct FailingHugeReleaseBackend;

static FAILING_HUGE_POOLS: BackendPools = BackendPools::new();
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

impl crate::segment::pool::private::Sealed for FailingHugeReleaseBackend {}

impl HasSegmentPool for FailingHugeReleaseBackend {
    fn pools() -> &'static BackendPools {
        &FAILING_HUGE_POOLS
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
        let user_ptr = unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(size, align, true) };
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
        let user_ptr = unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(4096, align, true) };
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
    unsafe {
        crate::segment::purge_segment_pool::<MemoryBackendWrapper>();
    }
    // The tight mapping formula reserves exactly
    // `size + SEGMENT_ALIGN + max(alignment, PAGE_SIZE)`. Verify the backend
    // counter observed precisely that increment, so future regressions
    // that re-introduce the SEGMENT_SIZE-of-slack would fail loudly.
    let size = 4 * 1024 * 1024;
    let align = 8;
    let expected = size + SEGMENT_ALIGN + core::cmp::max(align, PAGE_SIZE);

    let before = backend_memory_stats();
    // Safety: power-of-two alignment, non-zero size.
    let user_ptr = unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(size, align, true) };
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
    let released = unsafe { deallocate_large_or_huge::<MemoryBackendWrapper>(user_ptr, recovered) };
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
        unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(4096, SEGMENT_SIZE * 2, true) };
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
    let user_ptr = unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(0, 8, true) };
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
    let user_ptr =
        unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(MAX_ALLOC_SIZE, 8, true) };
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
        Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0);
        (*segment_ptr).pages[0].block_size = SEGMENT_SIZE * 10;
    }

    let released = unsafe {
        deallocate_large_or_huge::<FailingHugeReleaseBackend>(0x2000 as *mut u8, segment_ptr)
    };

    assert!(!released, "failing huge release backend reported success");
    assert_eq!(FAILING_HUGE_DEALLOC_CALLS.load(Ordering::Relaxed), 1);
}

struct DecommitRecordingHugeBackend;

static DECOMMIT_HUGE_POOLS: BackendPools = BackendPools::new();
static DECOMMIT_HUGE_CALLS: AtomicUsize = AtomicUsize::new(0);
static DECOMMIT_HUGE_BYTES: AtomicUsize = AtomicUsize::new(0);

impl MemoryBackend for DecommitRecordingHugeBackend {
    const SUPPORTS_DECOMMIT: bool = true;

    unsafe fn allocate(size: usize) -> *mut u8 {
        let layout = std::alloc::Layout::from_size_align(size, SEGMENT_ALIGN)
            .expect("huge mapping layout must be valid");
        unsafe { std::alloc::alloc(layout) }
    }

    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        let layout = std::alloc::Layout::from_size_align(size, SEGMENT_ALIGN)
            .expect("huge mapping layout must be valid");
        unsafe {
            std::alloc::dealloc(ptr, layout);
        }
        true
    }

    unsafe fn decommit(ptr: *mut u8, size: usize) -> bool {
        let _ = ptr;
        DECOMMIT_HUGE_CALLS.fetch_add(1, Ordering::Relaxed);
        DECOMMIT_HUGE_BYTES.fetch_add(size, Ordering::Relaxed);
        true
    }
}

impl crate::segment::pool::private::Sealed for DecommitRecordingHugeBackend {}

impl HasSegmentPool for DecommitRecordingHugeBackend {
    fn pools() -> &'static BackendPools {
        &DECOMMIT_HUGE_POOLS
    }
}

#[test]
fn huge_allocation_decommits_tail_slack() {
    let _guard = TEST_LOCK.lock().expect("arena test lock was poisoned");
    DECOMMIT_HUGE_CALLS.store(0, Ordering::Relaxed);
    DECOMMIT_HUGE_BYTES.store(0, Ordering::Relaxed);

    let size = 4 * 1024 * 1024;
    let align = 8;
    let user_ptr =
        unsafe { allocate_large_or_huge::<DecommitRecordingHugeBackend>(size, align, true) };
    assert!(!user_ptr.is_null());

    let calls = DECOMMIT_HUGE_CALLS.load(Ordering::Relaxed);
    let bytes = DECOMMIT_HUGE_BYTES.load(Ordering::Relaxed);

    assert!(
        calls >= 1,
        "Expected at least 1 decommit call, got {}",
        calls
    );
    assert!(
        bytes >= SEGMENT_SIZE,
        "Expected at least {} bytes decommitted, got {}",
        SEGMENT_SIZE,
        bytes
    );

    let recovered = unsafe { *((user_ptr as *mut *mut Segment).sub(1)) };
    let released =
        unsafe { deallocate_large_or_huge::<DecommitRecordingHugeBackend>(user_ptr, recovered) };
    assert!(released);
}

#[test]
fn test_huge_allocation_caching_and_purging() {
    let _guard = TEST_LOCK.lock().expect("arena test lock was poisoned");
    use mnemosyne_backend::{backend_memory_stats, MemoryBackendWrapper};

    // Clear any existing cached blocks in the pool
    unsafe {
        crate::segment::purge_segment_pool::<MemoryBackendWrapper>();
    }

    let size = 4 * 1024 * 1024;
    let align = 8;

    let stats_start = backend_memory_stats();

    // 1. First allocation: OS-backed
    let ptr1 = unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(size, align, false) };
    assert!(!ptr1.is_null());

    let stats_alloc1 = backend_memory_stats();
    assert!(stats_alloc1.current_mapped_bytes > stats_start.current_mapped_bytes);

    let segment1 = unsafe { *((ptr1 as *mut *mut Segment).sub(1)) };
    let raw_ptr1 = unsafe { (*segment1).raw_alloc_ptr };

    // Deallocate: pushes to cache (mapped bytes should NOT decrease)
    let released1 = unsafe { deallocate_large_or_huge::<MemoryBackendWrapper>(ptr1, segment1) };
    assert!(released1);

    let stats_dealloc1 = backend_memory_stats();
    assert_eq!(
        stats_dealloc1.current_mapped_bytes,
        stats_alloc1.current_mapped_bytes
    );

    // 2. Second allocation: should reuse the cached block (mapped bytes should NOT increase)
    let ptr2 = unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(size, align, false) };
    assert!(!ptr2.is_null());

    let stats_alloc2 = backend_memory_stats();
    assert_eq!(
        stats_alloc2.current_mapped_bytes,
        stats_alloc1.current_mapped_bytes
    );

    let segment2 = unsafe { *((ptr2 as *mut *mut Segment).sub(1)) };
    let raw_ptr2 = unsafe { (*segment2).raw_alloc_ptr };

    assert_eq!(
        raw_ptr2, raw_ptr1,
        "Second allocation did not reuse the cached block"
    );

    // Deallocate again
    let released2 = unsafe { deallocate_large_or_huge::<MemoryBackendWrapper>(ptr2, segment2) };
    assert!(released2);

    // 3. Purge: must free the cached block back to the OS (mapped bytes should decrease)
    unsafe {
        crate::segment::purge_segment_pool::<MemoryBackendWrapper>();
    }

    let stats_purged = backend_memory_stats();
    assert_eq!(
        stats_purged.current_mapped_bytes,
        stats_start.current_mapped_bytes
    );

    // 4. Third allocation: should be a fresh OS allocation
    let ptr3 = unsafe { allocate_large_or_huge::<MemoryBackendWrapper>(size, align, false) };
    assert!(!ptr3.is_null());

    let segment3 = unsafe { *((ptr3 as *mut *mut Segment).sub(1)) };

    // Clean up
    let released3 = unsafe { deallocate_large_or_huge::<MemoryBackendWrapper>(ptr3, segment3) };
    assert!(released3);
    unsafe {
        crate::segment::purge_segment_pool::<MemoryBackendWrapper>();
    }
}
