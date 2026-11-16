//! Segment telemetry tests: `decommit` slack is recorded on the active
//! backend.

#[cfg(feature = "segment-tail-guards")]
use super::alloc::SEGMENT_TAIL_GUARD_SIZE;
use super::alloc::{allocate_segment, release_segment_mapping};
use super::pool::{BackendPools, HasSegmentPool};
use super::stats::SegmentRelease;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::MemoryBackend;
use mnemosyne_core::constants::{SEGMENT_ALIGN, SEGMENT_SIZE};

struct DecommitRecordingBackend;

static DECOMMIT_POOLS: BackendPools = BackendPools::new();
static DECOMMIT_CALLS: AtomicUsize = AtomicUsize::new(0);
static DECOMMIT_BYTES: AtomicUsize = AtomicUsize::new(0);

impl MemoryBackend for DecommitRecordingBackend {
    const SUPPORTS_DECOMMIT: bool = true;

    unsafe fn allocate(size: usize) -> *mut u8 {
        let layout = std::alloc::Layout::from_size_align(size, SEGMENT_ALIGN)
            .expect("segment mapping layout must be valid");
        // SAFETY: `MemoryBackend::allocate` requires a non-zero `size`, so
        // `layout` has non-zero size as `std::alloc::alloc` requires. The
        // caller owns the returned block and releases it through
        // `deallocate` with the same `size`.
        unsafe { std::alloc::alloc(layout) }
    }

    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        let layout = std::alloc::Layout::from_size_align(size, SEGMENT_ALIGN)
            .expect("segment mapping layout must be valid");
        // SAFETY: `MemoryBackend::deallocate` requires `ptr` to come from
        // `allocate` with this `size`. `allocate` built its layout from the
        // same `size` and `SEGMENT_ALIGN`, so `layout` equals the layout the
        // block was allocated with.
        unsafe {
            std::alloc::dealloc(ptr, layout);
        }
        true
    }

    unsafe fn decommit(ptr: *mut u8, size: usize) -> bool {
        #[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
        {
            let page = super::alloc::guard::native_guard_size(1).expect("native page geometry");
            assert!(
                ptr.addr().is_multiple_of(page),
                "decommit address must be page aligned"
            );
            assert!(
                size.is_multiple_of(page),
                "decommit length must cover whole pages"
            );
        }
        #[cfg(not(any(feature = "segment-tail-guards", feature = "segment-header-guards")))]
        let _ = ptr;
        DECOMMIT_CALLS.fetch_add(1, Ordering::Relaxed);
        DECOMMIT_BYTES.fetch_add(size, Ordering::Relaxed);
        true
    }
}

impl super::pool::private::Sealed for DecommitRecordingBackend {}

impl HasSegmentPool for DecommitRecordingBackend {
    fn pools() -> &'static BackendPools {
        &DECOMMIT_POOLS
    }
}

#[test]
fn test_segment_tail_slack_decommit() {
    while DecommitRecordingBackend::global_segment_pool()
        .pop()
        .is_some()
    {}
    DECOMMIT_CALLS.store(0, Ordering::Relaxed);
    DECOMMIT_BYTES.store(0, Ordering::Relaxed);

    // SAFETY: `DecommitRecordingBackend` implements `HasSegmentPool` over its
    // own static pool, which the loop above emptied. The segment is therefore
    // freshly mapped from this backend, and this test is its sole owner until
    // it is released below.
    let segment =
        unsafe { allocate_segment::<DecommitRecordingBackend>() }.expect("segment allocation");

    let calls = DECOMMIT_CALLS.load(Ordering::Relaxed);
    let bytes = DECOMMIT_BYTES.load(Ordering::Relaxed);
    assert!(
        calls >= 1,
        "Expected at least 1 decommit call for slack memory, got {}",
        calls
    );
    #[cfg(not(feature = "segment-tail-guards"))]
    let guard_bytes = 0;
    #[cfg(feature = "segment-tail-guards")]
    let guard_bytes = super::alloc::guard::native_guard_size(SEGMENT_TAIL_GUARD_SIZE).unwrap_or(0);
    assert_eq!(
        bytes,
        SEGMENT_SIZE - guard_bytes,
        "all alignment slack outside the complete guard must be decommitted"
    );

    // SAFETY: `segment` was returned by `allocate_segment` for this same
    // backend, is initialized, has not been pushed to the pool, and no other
    // reference to it exists, so releasing its mapping is sound.
    let released = unsafe { release_segment_mapping::<DecommitRecordingBackend>(segment) };
    assert_eq!(released, SegmentRelease::Released);
}
