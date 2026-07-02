//! Unit tests for segment allocation and pool management.

extern crate std;

#[allow(unused_imports)]
use super::alloc::{
    SEGMENT_MAPPING_SIZE, SEGMENT_TAIL_GUARD_SIZE, allocate_segment, deallocate_segment,
    purge_segment_pool, release_segment_mapping, reset_segment_pool,
};
use super::pool::{BackendPools, GlobalHugePool, GlobalSegmentPool, HasSegmentPool};
use super::stats::{SegmentRelease, arena_memory_stats};
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::MemoryBackend;
use mnemosyne_core::constants::{PAGE_SIZE, SEGMENT_ALIGN, SEGMENT_SIZE};
use mnemosyne_core::types::Segment;
use std::boxed::Box;

#[cfg(any(feature = "segment-tail-guards", feature = "segment-header-guards"))]
use std::alloc::{Layout, alloc, dealloc};

struct FailingReleaseBackend;

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
            Segment::initialize(p, 0x1000 as *mut u8, 0);
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
        head = unsafe { (*head).next_free_segment };
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
    // Sizes beyond MAX_CACHED_HUGE_SIZE saturate to the last live bucket
    // (they are never pushed; only over-sized pop requests reach here).
    assert_eq!(huge_bucket_index(16 * 1024 * 1024 + 1), 10);
    assert_eq!(huge_bucket_index(512 * 1024 * 1024), 10);
}

#[test]
fn test_huge_pool_bucket_count_derived_from_max_cached_size() {
    use super::pool::huge_pool::{HUGE_SIZE_BUCKETS, huge_bucket_index};

    // SSOT pin: the bucket fan-out is exactly index(MAX_CACHED_HUGE_SIZE) + 1,
    // so no bucket is unreachable dead state under `try_push`'s size gate.
    assert_eq!(
        HUGE_SIZE_BUCKETS,
        huge_bucket_index(GlobalHugePool::MAX_CACHED_HUGE_SIZE) + 1
    );
    // 16 KiB (bucket 0) through 16 MiB (bucket 10) in log2 steps.
    assert_eq!(HUGE_SIZE_BUCKETS, 11);
}

#[test]
fn test_huge_bucket_block_cap_bounds_retained_bytes() {
    use super::pool::GlobalHugePool;
    use super::pool::huge_pool::{HUGE_SIZE_BUCKETS, bucket_block_cap};

    const BUDGET: usize = GlobalHugePool::MAX_CACHED_HUGE_BYTES_PER_BUCKET;

    // Small-huge buckets keep the full count cap (no perf regression there):
    // 256 MiB / 16 KiB = 16384, clamped to MAX_CACHED_HUGE_BLOCKS.
    assert_eq!(bucket_block_cap(0), GlobalHugePool::MAX_CACHED_HUGE_BLOCKS);
    // 1 MiB blocks (bucket 6): 256 MiB / 1 MiB = 256.
    assert_eq!(bucket_block_cap(6), 256);
    // 16 MiB blocks (last live bucket, 10): 256 MiB / 16 MiB = 16.
    assert_eq!(bucket_block_cap(HUGE_SIZE_BUCKETS - 1), 16);

    // Invariant: every live bucket's retained bytes (cap × max block size)
    // stay within the per-bucket budget, and never exceed the flat count cap.
    for idx in 0..HUGE_SIZE_BUCKETS {
        let cap = bucket_block_cap(idx);
        assert!((1..=GlobalHugePool::MAX_CACHED_HUGE_BLOCKS).contains(&cap));
        let max_block = 1usize << (idx + 14);
        assert!(
            cap * max_block <= BUDGET,
            "bucket {idx}: cap {cap} x {max_block} exceeds budget {BUDGET}"
        );
    }
}

/// Boxes a minimal `Segment` carrying only the page-0 `block_size` metadata
/// the huge-pool implementation reads.
///
/// The zeroed remainder is never exposed to allocator code that relies on the
/// full production `Segment::initialize` invariant.
fn boxed_huge_segment(raw: usize, block_size: usize) -> *mut Segment {
    let segment = Box::into_raw(Box::new(Segment {
        raw_alloc_ptr: raw as *mut u8,
        next_free_segment: core::ptr::null_mut(),
        // SAFETY: zeroed metadata is immediately overwritten where read (the
        // page-0 `block_size` below) and otherwise never interpreted.
        ..unsafe { core::mem::zeroed() }
    }));
    // SAFETY: `segment` is the live Box allocation just created above, so
    // mutating its page-0 size metadata through the raw pointer is exclusive.
    unsafe {
        (*segment).pages[0].block_size = block_size;
    }
    segment
}

#[test]
fn test_huge_pool_exact_bucket_restores_rejected_head() {
    let pool = GlobalHugePool::new();
    // All four blocks land in bucket 1 ((16 KiB, 32 KiB]). Pushing the fitting
    // block first buries it at the bottom of the LIFO stack under three
    // undersized rejects (top-down order after the pushes: c, b, a, fitting).
    let fitting = boxed_huge_segment(0x20000, 24 * 1024);
    let small_a = boxed_huge_segment(0x30000, 17 * 1024);
    let small_b = boxed_huge_segment(0x40000, 18 * 1024);
    let small_c = boxed_huge_segment(0x50000, 19 * 1024);

    unsafe {
        assert!(pool.try_push(fitting, 0), "fitting segment must be cached");
        for small in [small_a, small_b, small_c] {
            assert!(
                pool.try_push(small, 0),
                "undersized same-bucket segment must be cached"
            );
        }
    }
    assert_eq!(pool.retained_blocks(), 4);
    assert_eq!(pool.retained_bytes(), (24 + 17 + 18 + 19) * 1024);

    let popped = unsafe { pool.pop(20 * 1024, 0) }
        .expect("same-size bucket must scan past undersized heads");
    assert_eq!(popped, fitting);
    unsafe {
        assert_eq!((*popped).next_free_segment, core::ptr::null_mut());
    }

    // Count and byte conservation: exactly the three rejects remain cached.
    assert_eq!(pool.retained_blocks(), 3);
    assert_eq!(pool.retained_bytes(), (17 + 18 + 19) * 1024);

    // The rejected chain is spliced back in walk order, preserving the
    // original LIFO order (c, b, a): a request every reject satisfies must
    // pop them head-first in that exact order, proving each rejected segment
    // is still retrievable with intact size metadata and cleared links.
    for (expected, expected_size) in [
        (small_c, 19 * 1024),
        (small_b, 18 * 1024),
        (small_a, 17 * 1024),
    ] {
        let restored = unsafe { pool.pop(16 * 1024 + 1, 0) }
            .expect("rejected segment must be restored to the bucket");
        assert_eq!(restored, expected);
        unsafe {
            assert_eq!((*restored).pages[0].block_size, expected_size);
            assert_eq!((*restored).next_free_segment, core::ptr::null_mut());
        }
    }
    assert_eq!(pool.retained_blocks(), 0);
    assert_eq!(pool.retained_bytes(), 0);
    assert!(
        unsafe { pool.pop(16 * 1024, 0) }.is_none(),
        "no segment may remain after all rejects were drained"
    );

    for segment in [fitting, small_a, small_b, small_c] {
        unsafe {
            let _ = Box::from_raw(segment);
        }
    }
}

#[test]
fn test_huge_pool_pop_skips_over_provisioned_buckets() {
    use super::pool::huge_pool::HUGE_POP_FIT_CAP;

    let pool = GlobalHugePool::new();
    // Ground the test's size choices in the cap: bucket 10's exclusive lower
    // bound (8 MiB) is beyond HUGE_POP_FIT_CAP x 20 KiB (inadmissible), while
    // bucket 2's (32 KiB) is within it (admissible).
    const {
        assert!(8 * 1024 * 1024 >= HUGE_POP_FIT_CAP * 20 * 1024);
        assert!(32 * 1024 < HUGE_POP_FIT_CAP * 20 * 1024);
    }

    // A cached 16 MiB block (bucket 10) must NOT satisfy a ~20 KiB-class
    // request (bucket 1): bucket 10's smallest possible block (8 MiB + 1)
    // exceeds HUGE_POP_FIT_CAP (4) x 20 KiB, so the scan stops long before it
    // and the pop misses instead of over-provisioning ~800x.
    let oversized = boxed_huge_segment(0x60000, 16 * 1024 * 1024);
    unsafe {
        assert!(pool.try_push(oversized, 0), "16 MiB block must be cached");
    }
    assert!(
        unsafe { pool.pop(20 * 1024, 0) }.is_none(),
        "a block beyond the fit cap must miss, not over-provision"
    );
    // The miss leaves the oversized block cached.
    assert_eq!(pool.retained_blocks(), 1);
    assert_eq!(pool.retained_bytes(), 16 * 1024 * 1024);

    // A higher bucket within the cap still hits: a 64 KiB block (bucket 2)
    // serves a 20 KiB request because bucket 2's lower bound (32 KiB) is
    // below 4 x 20 KiB = 80 KiB.
    let medium = boxed_huge_segment(0x70000, 64 * 1024);
    unsafe {
        assert!(pool.try_push(medium, 0), "64 KiB block must be cached");
    }
    let popped = unsafe { pool.pop(20 * 1024, 0) }
        .expect("a higher bucket within the fit cap must still serve the request");
    assert_eq!(popped, medium);
    assert_eq!(pool.retained_blocks(), 1);
    assert_eq!(pool.retained_bytes(), 16 * 1024 * 1024);

    // The oversized block itself is retrievable by a request it fits within
    // the cap (16 MiB request, exact bucket).
    let reclaimed = unsafe { pool.pop(16 * 1024 * 1024, 0) }
        .expect("exact-bucket request must retrieve the 16 MiB block");
    assert_eq!(reclaimed, oversized);
    assert_eq!(pool.retained_blocks(), 0);
    assert_eq!(pool.retained_bytes(), 0);

    for segment in [oversized, medium] {
        unsafe {
            let _ = Box::from_raw(segment);
        }
    }
}

#[test]
fn test_arena_stats_report_runtime_retained_cap() {
    use mnemosyne_core::options::{MnemosyneOptions, set_options};

    // Lower the runtime cap below the compile-time limit: the stat must track
    // the enforced runtime value (what `try_push_retained` reads), not the
    // compile-time `MAX_RETAINED_SEGMENTS_LIMIT`.
    set_options(MnemosyneOptions {
        max_retained_segments: 7,
        ..Default::default()
    });
    let stats = arena_memory_stats::<FailingReleaseBackend>();
    assert_eq!(stats.max_retained_free_segments, 7);

    // Restore the default; the stat must follow (the default option equals
    // the compile-time limit).
    set_options(MnemosyneOptions::default());
    let stats = arena_memory_stats::<FailingReleaseBackend>();
    assert_eq!(
        stats.max_retained_free_segments,
        mnemosyne_core::constants::MAX_RETAINED_SEGMENTS_LIMIT
    );
}

#[test]
fn test_arena_stats_track_huge_pool_blocks_and_bytes() {
    let before = arena_memory_stats::<FailingReleaseBackend>();

    let block = boxed_huge_segment(0x90000, 24 * 1024);
    unsafe {
        assert!(
            FailingReleaseBackend::global_huge_pool().try_push(block, 0),
            "huge block must be cached"
        );
    }
    let during = arena_memory_stats::<FailingReleaseBackend>();
    assert_eq!(during.retained_huge_blocks, before.retained_huge_blocks + 1);
    assert_eq!(
        during.retained_huge_bytes,
        before.retained_huge_bytes + 24 * 1024
    );

    let popped = unsafe { FailingReleaseBackend::global_huge_pool().pop(24 * 1024, 0) }
        .expect("cached huge block must be retrievable");
    assert_eq!(popped, block);
    let after = arena_memory_stats::<FailingReleaseBackend>();
    assert_eq!(after.retained_huge_blocks, before.retained_huge_blocks);
    assert_eq!(after.retained_huge_bytes, before.retained_huge_bytes);

    unsafe {
        let _ = Box::from_raw(block);
    }
}
