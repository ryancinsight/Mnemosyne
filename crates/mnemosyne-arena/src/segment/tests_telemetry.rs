//! Segment telemetry tests: `decommit` slack and `page_reset` bounds are
//! recorded on the active backend and propagated to the segment pool.

use super::alloc::{
    allocate_segment, deallocate_segment, release_segment_mapping, reset_segment_pool,
};
use super::pool::{BackendPools, HasSegmentPool};
use super::stats::SegmentRelease;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::MemoryBackend;
use mnemosyne_core::constants::{PAGE_SIZE, SEGMENT_ALIGN, SEGMENT_SIZE};

struct DecommitRecordingBackend;

static DECOMMIT_POOLS: BackendPools = BackendPools::new();
static DECOMMIT_CALLS: AtomicUsize = AtomicUsize::new(0);
static DECOMMIT_BYTES: AtomicUsize = AtomicUsize::new(0);

impl MemoryBackend for DecommitRecordingBackend {
    const SUPPORTS_DECOMMIT: bool = true;

    unsafe fn allocate(size: usize) -> *mut u8 {
        let layout = std::alloc::Layout::from_size_align(size, SEGMENT_ALIGN)
            .expect("segment mapping layout must be valid");
        unsafe { std::alloc::alloc(layout) }
    }

    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        let layout = std::alloc::Layout::from_size_align(size, SEGMENT_ALIGN)
            .expect("segment mapping layout must be valid");
        unsafe {
            std::alloc::dealloc(ptr, layout);
        }
        true
    }

    unsafe fn decommit(ptr: *mut u8, size: usize) -> bool {
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
    while DecommitRecordingBackend::global_orphan_pool()
        .pop()
        .is_some()
    {}
    DECOMMIT_CALLS.store(0, Ordering::Relaxed);
    DECOMMIT_BYTES.store(0, Ordering::Relaxed);

    let segment =
        unsafe { allocate_segment::<DecommitRecordingBackend>() }.expect("segment allocation");

    let calls = DECOMMIT_CALLS.load(Ordering::Relaxed);
    let bytes = DECOMMIT_BYTES.load(Ordering::Relaxed);
    assert!(
        calls >= 1,
        "Expected at least 1 decommit call for slack memory, got {}",
        calls
    );
    assert!(
        bytes >= SEGMENT_SIZE - 4096,
        "Expected at least {} bytes decommitted, got {}",
        SEGMENT_SIZE - 4096,
        bytes
    );

    let released = unsafe { release_segment_mapping::<DecommitRecordingBackend>(segment) };
    assert_eq!(released, SegmentRelease::Released);
}

struct ResetRecordingBackend;

static RESET_POOLS: BackendPools = BackendPools::new();
static RESET_CALLS: AtomicUsize = AtomicUsize::new(0);
static LAST_RESET_PTR: AtomicUsize = AtomicUsize::new(0);
static LAST_RESET_SIZE: AtomicUsize = AtomicUsize::new(0);

impl MemoryBackend for ResetRecordingBackend {
    const SUPPORTS_PAGE_RESET: bool = true;

    unsafe fn allocate(size: usize) -> *mut u8 {
        let layout = std::alloc::Layout::from_size_align(size, SEGMENT_ALIGN)
            .expect("segment mapping layout must be valid");
        unsafe { std::alloc::alloc(layout) }
    }

    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        let layout = std::alloc::Layout::from_size_align(size, SEGMENT_ALIGN)
            .expect("segment mapping layout must be valid");
        unsafe {
            std::alloc::dealloc(ptr, layout);
        }
        true
    }

    unsafe fn page_reset(ptr: *mut u8, size: usize) -> bool {
        RESET_CALLS.fetch_add(1, Ordering::Relaxed);
        LAST_RESET_PTR.store(ptr as usize, Ordering::Relaxed);
        LAST_RESET_SIZE.store(size, Ordering::Relaxed);
        true
    }
}

impl super::pool::private::Sealed for ResetRecordingBackend {}

impl HasSegmentPool for ResetRecordingBackend {
    fn pools() -> &'static BackendPools {
        &RESET_POOLS
    }
}

#[test]
fn test_reset_segment_pool_propagates_correct_bounds() {
    use mnemosyne_core::options::{MnemosyneOptions, set_options};

    // This test needs the pool to actually retain the freed segment, so it
    // establishes retention rather than inheriting whatever the build's
    // default happens to be. Under Miri that default is deliberately 0
    // (options.rs suppresses retention to keep leak evidence focused), which
    // would leave nothing for `reset_segment_pool` to reset and fail the
    // call-count assertion without any defect being present.
    set_options(MnemosyneOptions {
        max_retained_segments: mnemosyne_core::constants::MAX_RETAINED_SEGMENTS_LIMIT,
        ..Default::default()
    });

    while ResetRecordingBackend::global_segment_pool().pop().is_some() {}
    while ResetRecordingBackend::global_orphan_pool().pop().is_some() {}
    RESET_CALLS.store(0, Ordering::Relaxed);
    LAST_RESET_PTR.store(0, Ordering::Relaxed);
    LAST_RESET_SIZE.store(0, Ordering::Relaxed);

    let segment =
        unsafe { allocate_segment::<ResetRecordingBackend>() }.expect("segment allocation");

    // Push it back to the pool to make it eligible for reset
    unsafe {
        deallocate_segment::<ResetRecordingBackend>(segment);
    }

    unsafe {
        reset_segment_pool::<ResetRecordingBackend>();
    }

    let calls = RESET_CALLS.load(Ordering::Relaxed);
    let last_ptr = LAST_RESET_PTR.load(Ordering::Relaxed);
    let last_size = LAST_RESET_SIZE.load(Ordering::Relaxed);

    assert_eq!(calls, 1, "expected exactly 1 page_reset call");
    assert_eq!(
        last_ptr,
        segment as usize + PAGE_SIZE,
        "expected page_reset pointer to match segment pointer plus PAGE_SIZE"
    );
    assert_eq!(
        last_size,
        SEGMENT_SIZE - PAGE_SIZE,
        "expected page_reset size to match SEGMENT_SIZE minus PAGE_SIZE"
    );

    // Clean up
    let popped = ResetRecordingBackend::global_segment_pool()
        .pop()
        .expect("segment must be in the pool");
    let released = unsafe { release_segment_mapping::<ResetRecordingBackend>(popped) };
    assert_eq!(released, SegmentRelease::Released);

    // Restore the build's default so this test leaves no global option state
    // behind for anything sharing the process.
    set_options(MnemosyneOptions::default());
}
