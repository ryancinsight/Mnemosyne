//! Unit tests for segment allocation and pool management.

extern crate std;

#[expect(unused_imports)]
use super::alloc::{
    SEGMENT_MAPPING_SIZE, SEGMENT_TAIL_GUARD_SIZE, allocate_segment, deallocate_segment,
    purge_segment_pool, release_segment_mapping, reset_segment_pool,
};
use super::pool::{BackendPools, GlobalSegmentPool, HasSegmentPool};
#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
use super::stats::SegmentRelease;
use super::stats::arena_memory_stats;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::MemoryBackend;
use mnemosyne_core::types::Segment;
use std::boxed::Box;

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
use mnemosyne_core::constants::{PAGE_SIZE, SEGMENT_ALIGN, SEGMENT_SIZE};

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
use std::alloc::{Layout, alloc, dealloc};

pub(super) struct FailingReleaseBackend;

static FAILING_POOLS: BackendPools = BackendPools::new();
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
    fn pools() -> &'static BackendPools {
        &FAILING_POOLS
    }
}

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
struct GuardRecordingBackend;

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
static GUARD_POOLS: BackendPools = BackendPools::new();
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
    fn pools() -> &'static BackendPools {
        &GUARD_POOLS
    }
}

#[test]
fn purge_retains_segment_when_backend_release_fails() {
    let mut segment = core::mem::MaybeUninit::<Segment>::uninit();
    let segment_ptr = segment.as_mut_ptr();

    unsafe {
        Segment::initialize(segment_ptr, segment_ptr.cast(), 0);
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

#[test]
fn node_segment_pool_take_all_detaches_whole_chain_in_one_lock() {
    use crate::segment::pool::list::NodeSegmentPool;

    let pool = NodeSegmentPool::new();
    let mut segs = [
        core::mem::MaybeUninit::<Segment>::uninit(),
        core::mem::MaybeUninit::<Segment>::uninit(),
        core::mem::MaybeUninit::<Segment>::uninit(),
    ];
    for s in segs.iter_mut() {
        let p = s.as_mut_ptr();
        // SAFETY: `p` is a unique stack slot; `push_unbounded` only threads it
        // onto the pool's intrusive list. The segments are never released (the
        // test drops the pool without deallocating), so stack storage is fine.
        unsafe {
            Segment::initialize(p, p.cast(), 0);
            pool.push_unbounded(p);
        }
    }
    assert_eq!(pool.retained_count(), 3);

    let (mut head, count) = pool.take_all();
    assert_eq!(count, 3, "take_all must report the detached count");
    assert_eq!(
        pool.retained_count(),
        0,
        "pool must be empty after take_all"
    );

    let mut walked = 0usize;
    while !head.is_null() {
        walked += 1;
        // SAFETY: `head` is a node of the chain just detached from `pool`.
        head = unsafe {
            (*head)
                .next_free_segment
                .load(core::sync::atomic::Ordering::Relaxed)
        };
    }
    assert_eq!(
        walked, 3,
        "detached chain must contain every pushed segment"
    );

    let (empty_head, empty_count) = pool.take_all();
    assert!(empty_head.is_null());
    assert_eq!(empty_count, 0);
}

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
#[test]
fn fresh_segment_install_increments_guard_telemetry_and_round_trips() {
    use mnemosyne_backend::{MemoryBackendWrapper, backend_memory_stats};

    // Purge to force the OS allocation path.
    unsafe {
        purge_segment_pool::<MemoryBackendWrapper>();
    }
    let before = backend_memory_stats();

    // SAFETY: arena-managed segment allocation.
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
        // SAFETY: an all-zero bit pattern is a valid starting value for the
        // initializer to overwrite.
        let seg_ptr: *mut Segment = Box::into_raw(Box::new(unsafe { core::mem::zeroed() }));
        // SAFETY: `seg_ptr` is the live Box allocation just created.
        unsafe { Segment::initialize(seg_ptr, raw, 0) };
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
