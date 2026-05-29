//! Unit tests for segment allocation and pool management.

extern crate std;

#[allow(unused_imports)]
use super::alloc::{
    allocate_segment, deallocate_segment, purge_segment_pool, release_segment_mapping,
    SEGMENT_MAPPING_SIZE, SEGMENT_TAIL_GUARD_SIZE,
};
use super::pool::{GlobalSegmentPool, HasSegmentPool};
use super::stats::{arena_memory_stats, SegmentRelease};
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::constants::{SEGMENT_ALIGN, SEGMENT_SIZE};
use mnemosyne_core::types::Segment;
use mnemosyne_core::MemoryBackend;
use std::boxed::Box;

#[cfg(feature = "segment-tail-guards")]
use std::alloc::{alloc, dealloc, Layout};

struct FailingReleaseBackend;

static FAILING_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static FAILING_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static FAILING_HUGE_POOL_TEST: crate::huge_pool::HugeMappingPool = crate::huge_pool::HugeMappingPool::new();
static FAILING_DEALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);

impl MemoryBackend for FailingReleaseBackend {
    unsafe fn allocate(_size: usize) -> *mut u8 {
        core::ptr::null_mut()
    }

    unsafe fn deallocate(_ptr: *mut u8, _size: usize) -> bool {
        FAILING_DEALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        false
    }
}

impl super::pool::private::Sealed for FailingReleaseBackend {}

impl HasSegmentPool for FailingReleaseBackend {
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &FAILING_POOL
    }

    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &FAILING_ORPHAN_POOL
    }

    fn global_huge_pool() -> &'static crate::huge_pool::HugeMappingPool {
        &FAILING_HUGE_POOL_TEST
    }
}

#[cfg(feature = "segment-tail-guards")]
struct GuardRecordingBackend;

#[cfg(feature = "segment-tail-guards")]
static GUARD_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
#[cfg(feature = "segment-tail-guards")]
static GUARD_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
#[cfg(feature = "segment-tail-guards")]
static GUARD_HUGE_POOL_TEST: crate::huge_pool::HugeMappingPool = crate::huge_pool::HugeMappingPool::new();
#[cfg(feature = "segment-tail-guards")]
static GUARD_CALLS: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "segment-tail-guards")]
static LAST_GUARD_PTR: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "segment-tail-guards")]
static LAST_GUARD_SIZE: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "segment-tail-guards")]
impl MemoryBackend for GuardRecordingBackend {
    const SUPPORTS_MAKE_GUARD: bool = true;

    unsafe fn allocate(size: usize) -> *mut u8 {
        let layout = Layout::from_size_align(size, SEGMENT_ALIGN)
            .expect("segment mapping layout must be valid");
        unsafe { alloc(layout) }
    }

    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        let layout = Layout::from_size_align(size, SEGMENT_ALIGN)
            .expect("segment mapping layout must be valid");
        unsafe {
            dealloc(ptr, layout);
        }
        true
    }

    unsafe fn make_guard(ptr: *mut u8, size: usize) -> bool {
        GUARD_CALLS.fetch_add(1, Ordering::Relaxed);
        LAST_GUARD_PTR.store(ptr as usize, Ordering::Relaxed);
        LAST_GUARD_SIZE.store(size, Ordering::Relaxed);
        true
    }
}

#[cfg(feature = "segment-tail-guards")]
impl super::pool::private::Sealed for GuardRecordingBackend {}

#[cfg(feature = "segment-tail-guards")]
impl HasSegmentPool for GuardRecordingBackend {
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &GUARD_POOL
    }

    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &GUARD_ORPHAN_POOL
    }

    fn global_huge_pool() -> &'static crate::huge_pool::HugeMappingPool {
        &GUARD_HUGE_POOL_TEST
    }
}

#[test]
fn purge_retains_segment_when_backend_release_fails() {
    let mut segment = core::mem::MaybeUninit::<Segment>::uninit();
    let segment_ptr = segment.as_mut_ptr();

    unsafe {
        Segment::initialize(segment_ptr, 0x1000 as *mut u8, 0);
        FailingReleaseBackend::global_segment_pool().push_unbounded(segment_ptr);
    }

    let before = arena_memory_stats::<FailingReleaseBackend>();
    unsafe {
        purge_segment_pool::<FailingReleaseBackend>();
    }
    let after = arena_memory_stats::<FailingReleaseBackend>();

    assert_eq!(after.retained_free_segments, before.retained_free_segments);
    assert_eq!(after.purge_calls, before.purge_calls + 1);
    assert_eq!(after.purged_segments, before.purged_segments);
    assert_eq!(after.purged_bytes, before.purged_bytes);
    assert_eq!(FAILING_DEALLOC_CALLS.load(Ordering::Relaxed), 1);

    assert!(
        FailingReleaseBackend::global_segment_pool().pop().is_some(),
        "failed release segment was not retained in the pool"
    );
}

#[cfg(feature = "segment-tail-guards")]
#[test]
fn fresh_segment_install_increments_guard_telemetry_and_round_trips() {
    use mnemosyne_backend::{backend_memory_stats, MemoryBackendWrapper};

    // Purge to force the OS allocation path.
    unsafe {
        purge_segment_pool::<MemoryBackendWrapper>();
    }
    let before = backend_memory_stats();

    // Safety: arena-managed segment allocation.
    let segment = unsafe { allocate_segment::<MemoryBackendWrapper>() }
        .expect("OS-backed segment allocation must succeed");
    let after_alloc = backend_memory_stats();

    if after_alloc.guard_install_calls > before.guard_install_calls {
        assert!(
            after_alloc.guard_install_bytes >= before.guard_install_bytes + SEGMENT_TAIL_GUARD_SIZE,
            "guard_install_bytes advanced by less than SEGMENT_TAIL_GUARD_SIZE"
        );
    }
    assert!(
        after_alloc.current_mapped_bytes >= before.current_mapped_bytes + SEGMENT_MAPPING_SIZE,
        "current_mapped_bytes did not advance by the full segment mapping"
    );

    unsafe {
        deallocate_segment::<MemoryBackendWrapper>(segment);
        purge_segment_pool::<MemoryBackendWrapper>();
    }
    let after_release = backend_memory_stats();
    assert_eq!(
        after_release.current_mapped_bytes, before.current_mapped_bytes,
        "current_mapped_bytes did not return to baseline after release"
    );
}

#[cfg(feature = "segment-tail-guards")]
#[test]
fn fresh_segment_installs_tail_guard_in_alignment_slack() {
    while GuardRecordingBackend::global_segment_pool().pop().is_some() {}
    while GuardRecordingBackend::global_orphan_pool().pop().is_some() {}
    GUARD_CALLS.store(0, Ordering::Relaxed);
    LAST_GUARD_PTR.store(0, Ordering::Relaxed);
    LAST_GUARD_SIZE.store(0, Ordering::Relaxed);

    let segment =
        unsafe { allocate_segment::<GuardRecordingBackend>() }.expect("segment allocation");
    let expected_guard = segment as usize + SEGMENT_SIZE;

    assert_eq!(
        GUARD_CALLS.load(Ordering::Relaxed),
        1,
        "fresh segment allocation did not request exactly one guard install"
    );
    assert_eq!(
        LAST_GUARD_PTR.load(Ordering::Relaxed),
        expected_guard,
        "tail guard was not placed immediately after the segment"
    );
    assert_eq!(
        LAST_GUARD_SIZE.load(Ordering::Relaxed),
        SEGMENT_TAIL_GUARD_SIZE,
        "tail guard size drifted from the documented constant"
    );

    let released = unsafe { release_segment_mapping::<GuardRecordingBackend>(segment) };
    assert_eq!(released, SegmentRelease::Released);
}

#[test]
fn test_concurrent_aba_safeness() {
    use std::sync::Arc;
    use std::sync::Barrier;
    use std::thread;

    let pool = Arc::new(GlobalSegmentPool::new());
    let barrier = Arc::new(Barrier::new(4));

    let mut segments = std::vec::Vec::new();
    for i in 0..10 {
        let raw = (0x10000 + i * 0x1000) as *mut u8;
        let seg_ptr = Box::into_raw(Box::new(Segment {
            raw_alloc_ptr: raw,
            next_free_segment: core::ptr::null_mut(),
            ..unsafe { core::mem::zeroed() }
        }));
        segments.push(seg_ptr);
    }

    for &seg in &segments {
        unsafe { pool.push_unbounded(seg) };
    }

    let mut handles = std::vec::Vec::new();
    for _ in 0..4 {
        let pool_clone = Arc::clone(&pool);
        let barrier_clone = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier_clone.wait();
            for _ in 0..2000 {
                if let Some(seg) = pool_clone.pop() {
                    unsafe { pool_clone.push_unbounded(seg) };
                }
            }
        }));
    }

    for h in handles {
        h.join().expect("thread failed");
    }

    // Clean up dummy segments
    while let Some(seg) = pool.pop() {
        unsafe {
            let _ = Box::from_raw(seg);
        }
    }
}

struct DecommitRecordingBackend;

static DECOMMIT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static DECOMMIT_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static DECOMMIT_HUGE_POOL_TEST: crate::huge_pool::HugeMappingPool = crate::huge_pool::HugeMappingPool::new();
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
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &DECOMMIT_POOL
    }

    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &DECOMMIT_ORPHAN_POOL
    }

    fn global_huge_pool() -> &'static crate::huge_pool::HugeMappingPool {
        &DECOMMIT_HUGE_POOL_TEST
    }
}

#[test]
fn test_segment_tail_slack_decommit() {
    while DECOMMIT_POOL.pop().is_some() {}
    while DECOMMIT_ORPHAN_POOL.pop().is_some() {}
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
