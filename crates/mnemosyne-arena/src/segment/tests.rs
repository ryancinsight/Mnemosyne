//! Unit tests for segment allocation and pool management.

extern crate std;

#[allow(unused_imports)]
use super::alloc::{
    allocate_segment, deallocate_segment, purge_segment_pool, release_segment_mapping,
    reset_segment_pool, SEGMENT_MAPPING_SIZE, SEGMENT_TAIL_GUARD_SIZE,
};
use super::pool::{GlobalHugePool, GlobalSegmentPool, HasSegmentPool};
use super::stats::{arena_memory_stats, SegmentRelease};
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::constants::{PAGE_SIZE, SEGMENT_ALIGN, SEGMENT_SIZE};
use mnemosyne_core::types::Segment;
use mnemosyne_core::MemoryBackend;
use std::boxed::Box;

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
use std::alloc::{alloc, dealloc, Layout};

struct FailingReleaseBackend;

static FAILING_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static FAILING_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static FAILING_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();
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

    fn global_huge_pool() -> &'static GlobalHugePool {
        &FAILING_HUGE_POOL
    }
}

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
struct GuardRecordingBackend;

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
static GUARD_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
static GUARD_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
static GUARD_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();
#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
static GUARD_CALLS: AtomicUsize = AtomicUsize::new(0);
#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
static LAST_GUARD_PTR: AtomicUsize = AtomicUsize::new(0);
#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
static LAST_GUARD_SIZE: AtomicUsize = AtomicUsize::new(0);

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
static GUARD_PTRS: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];
#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
static GUARD_SIZES: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
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
        let idx = GUARD_CALLS.fetch_add(1, Ordering::Relaxed);
        if idx < 2 {
            GUARD_PTRS[idx].store(ptr as usize, Ordering::Relaxed);
            GUARD_SIZES[idx].store(size, Ordering::Relaxed);
        }
        LAST_GUARD_PTR.store(ptr as usize, Ordering::Relaxed);
        LAST_GUARD_SIZE.store(size, Ordering::Relaxed);
        true
    }
}

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
impl super::pool::private::Sealed for GuardRecordingBackend {}

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
impl HasSegmentPool for GuardRecordingBackend {
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &GUARD_POOL
    }

    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &GUARD_ORPHAN_POOL
    }

    fn global_huge_pool() -> &'static GlobalHugePool {
        &GUARD_HUGE_POOL
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

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
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

    let mut expected_guards_size = 0;
    #[cfg(feature = "segment-tail-guards")]
    {
        expected_guards_size += SEGMENT_TAIL_GUARD_SIZE;
    }
    #[cfg(feature = "segment-header-guards")]
    {
        expected_guards_size += super::alloc::SEGMENT_HEADER_GUARD_SIZE;
    }

    if after_alloc.guard_install_calls > before.guard_install_calls {
        assert!(
            after_alloc.guard_install_bytes >= before.guard_install_bytes + expected_guards_size,
            "guard_install_bytes advanced by less than expected_guards_size"
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
    for i in 0..2 {
        GUARD_PTRS[i].store(0, Ordering::Relaxed);
        GUARD_SIZES[i].store(0, Ordering::Relaxed);
    }

    let segment =
        unsafe { allocate_segment::<GuardRecordingBackend>() }.expect("segment allocation");
    let expected_guard = segment as usize + SEGMENT_SIZE;

    // Find the tail guard in the recorded guard calls.
    let mut found = false;
    let limit = core::cmp::min(GUARD_CALLS.load(Ordering::Relaxed), 2);
    for idx in 0..limit {
        let ptr = GUARD_PTRS[idx].load(Ordering::Relaxed);
        let size = GUARD_SIZES[idx].load(Ordering::Relaxed);
        if ptr == expected_guard && size == SEGMENT_TAIL_GUARD_SIZE {
            found = true;
            break;
        }
    }

    assert!(
        found,
        "tail guard was not placed immediately after the segment"
    );

    let released = unsafe { release_segment_mapping::<GuardRecordingBackend>(segment) };
    assert_eq!(released, SegmentRelease::Released);
}

#[cfg(feature = "segment-header-guards")]
#[test]
fn fresh_segment_installs_header_guard_in_page_0() {
    while GuardRecordingBackend::global_segment_pool().pop().is_some() {}
    while GuardRecordingBackend::global_orphan_pool().pop().is_some() {}
    GUARD_CALLS.store(0, Ordering::Relaxed);
    for i in 0..2 {
        GUARD_PTRS[i].store(0, Ordering::Relaxed);
        GUARD_SIZES[i].store(0, Ordering::Relaxed);
    }

    let segment =
        unsafe { allocate_segment::<GuardRecordingBackend>() }.expect("segment allocation");
    let expected_guard = segment as usize + PAGE_SIZE - super::alloc::SEGMENT_HEADER_GUARD_SIZE;

    // Find the header guard in the recorded guard calls.
    let mut found = false;
    let limit = core::cmp::min(GUARD_CALLS.load(Ordering::Relaxed), 2);
    for idx in 0..limit {
        let ptr = GUARD_PTRS[idx].load(Ordering::Relaxed);
        let size = GUARD_SIZES[idx].load(Ordering::Relaxed);
        if ptr == expected_guard && size == super::alloc::SEGMENT_HEADER_GUARD_SIZE {
            found = true;
            break;
        }
    }

    assert!(found, "header guard was not placed at the end of Page 0");

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
static DECOMMIT_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();
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

    fn global_huge_pool() -> &'static GlobalHugePool {
        &DECOMMIT_HUGE_POOL
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

struct ResetRecordingBackend;

static RESET_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static RESET_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static RESET_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();
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
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &RESET_POOL
    }

    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &RESET_ORPHAN_POOL
    }

    fn global_huge_pool() -> &'static GlobalHugePool {
        &RESET_HUGE_POOL
    }
}

#[test]
fn test_reset_segment_pool_propagates_correct_bounds() {
    while RESET_POOL.pop().is_some() {}
    while RESET_ORPHAN_POOL.pop().is_some() {}
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
    let popped = RESET_POOL.pop().expect("segment must be in the pool");
    let released = unsafe { release_segment_mapping::<ResetRecordingBackend>(popped) };
    assert_eq!(released, SegmentRelease::Released);
}

#[test]
fn test_huge_pool_log2_bucketing() {
    use super::pool::huge_pool::huge_bucket_index;

    // Boundary at 16 KiB
    assert_eq!(huge_bucket_index(0), 0);
    assert_eq!(huge_bucket_index(16384), 0);
    assert_eq!(huge_bucket_index(16385), 1);

    // Power of two transitions
    assert_eq!(huge_bucket_index(32768), 1); // 32 KiB
    assert_eq!(huge_bucket_index(32769), 2);
    assert_eq!(huge_bucket_index(65536), 2); // 64 KiB
    assert_eq!(huge_bucket_index(65537), 3);
    assert_eq!(huge_bucket_index(1048576), 6); // 1 MiB
    assert_eq!(huge_bucket_index(1048577), 7);
    assert_eq!(huge_bucket_index(16 * 1024 * 1024), 10); // 16 MiB
    assert_eq!(huge_bucket_index(16 * 1024 * 1024 + 1), 11);
    assert_eq!(huge_bucket_index(512 * 1024 * 1024), 15); // Large sizes saturate to max bucket
}
