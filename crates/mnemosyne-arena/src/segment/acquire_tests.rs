//! Regression tests for the free/orphan split between [`allocate_segment`] and
//! [`acquire_segment`].
//!
//! An orphan is a segment whose owning thread cache exited while some of its
//! blocks were still live. Handing one out as if it were empty lets the caller
//! return it through `deallocate_segment` into the free pool, where a purge
//! unmaps the live blocks or a later allocation re-initializes them.

extern crate std;

use super::alloc::{
    AcquiredSegment, acquire_segment, allocate_segment, deallocate_segment, purge_segment_pool,
};
use super::pool::{BackendPools, HasSegmentPool};
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::MemoryBackend;
use mnemosyne_core::constants::SEGMENT_ALIGN;
use mnemosyne_core::types::Segment;
use std::sync::Mutex;

/// Heap-backed segment mappings that count releases of one watched mapping.
struct OrphanWatchBackend;

static ORPHAN_WATCH_POOLS: BackendPools = BackendPools::new();
/// Base address of the orphan's mapping; zero while nothing is watched.
static WATCHED_MAPPING: AtomicUsize = AtomicUsize::new(0);
/// Times the watched mapping was handed back to the backend.
static WATCHED_RELEASES: AtomicUsize = AtomicUsize::new(0);
/// Serializes the tests: they share the backend's pools and counters.
static ORPHAN_WATCH_LOCK: Mutex<()> = Mutex::new(());

impl MemoryBackend for OrphanWatchBackend {
    unsafe fn allocate(size: usize) -> *mut u8 {
        let layout = std::alloc::Layout::from_size_align(size, SEGMENT_ALIGN)
            .expect("invariant: SEGMENT_ALIGN is a power of two and mappings are non-empty");
        // SAFETY: the layout has a non-zero size.
        unsafe { std::alloc::alloc(layout) }
    }

    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        if ptr as usize == WATCHED_MAPPING.load(Ordering::Relaxed) {
            // The watched mapping backs live blocks: record the release and
            // keep the memory, so the test observes the defect as a count
            // rather than as a use-after-free.
            WATCHED_RELEASES.fetch_add(1, Ordering::Relaxed);
            return true;
        }
        let layout = std::alloc::Layout::from_size_align(size, SEGMENT_ALIGN)
            .expect("invariant: SEGMENT_ALIGN is a power of two and mappings are non-empty");
        // SAFETY: `ptr` came from `allocate` with this same layout.
        unsafe { std::alloc::dealloc(ptr, layout) };
        true
    }
}

impl super::pool::private::Sealed for OrphanWatchBackend {}

impl HasSegmentPool for OrphanWatchBackend {
    fn pools() -> &'static BackendPools {
        &ORPHAN_WATCH_POOLS
    }
}

/// Live blocks the fabricated orphan's first page reports.
const ORPHAN_LIVE_BLOCKS: u32 = 3;
/// Block size of the fabricated orphan's first page.
const ORPHAN_BLOCK_SIZE: u32 = 64;

/// Maps a segment, marks page 1 as holding live blocks (the state thread
/// teardown leaves when a block outlives its allocating thread), and parks it
/// in the orphan pool.
fn park_orphan_with_live_blocks() -> *mut Segment {
    // SAFETY: the backend's pools hold only segments this module created.
    let orphan = unsafe { allocate_segment::<OrphanWatchBackend>() }
        .expect("heap-backed segment mapping must succeed");
    // SAFETY: `orphan` is an initialized, exclusively-owned segment, and page
    // 1 is inside its page array.
    unsafe {
        (*orphan).pages[1].block_size = ORPHAN_BLOCK_SIZE;
        (*orphan).pages[1].alloc_count = ORPHAN_LIVE_BLOCKS;
        (*orphan).page_occupied_mask |= 1 << 1;
        WATCHED_MAPPING.store((*orphan).raw_alloc_ptr as usize, Ordering::Relaxed);
        OrphanWatchBackend::global_orphan_pool().push_unbounded(orphan);
    }
    orphan
}

/// Empties both pools, releasing the orphan's mapping, and stops watching it.
fn drain_pools() {
    WATCHED_MAPPING.store(0, Ordering::Relaxed);
    while let Some(segment) = OrphanWatchBackend::global_orphan_pool().pop() {
        // SAFETY: the popped segment is exclusively owned and its blocks are
        // fabricated, so releasing the mapping invalidates no reader.
        unsafe { deallocate_segment::<OrphanWatchBackend>(segment) };
    }
    // SAFETY: every retained segment is exclusively pool-owned.
    unsafe { purge_segment_pool::<OrphanWatchBackend>() };
}

#[test]
fn allocate_deallocate_purge_round_trip_never_releases_a_live_orphan() {
    let _guard = ORPHAN_WATCH_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    drain_pools();
    WATCHED_RELEASES.store(0, Ordering::Relaxed);
    let orphan = park_orphan_with_live_blocks();

    // The sequence the global-allocator purge test runs: take a segment, give
    // it back, purge the free pool.
    // SAFETY: the returned segment is released exactly once below.
    let segment = unsafe { allocate_segment::<OrphanWatchBackend>() }
        .expect("heap-backed segment mapping must succeed");
    let handed_out_orphan = segment == orphan;
    // SAFETY: `allocate_segment` returned an exclusively-owned segment.
    unsafe {
        deallocate_segment::<OrphanWatchBackend>(segment);
        purge_segment_pool::<OrphanWatchBackend>();
    }

    let releases = WATCHED_RELEASES.load(Ordering::Relaxed);
    let still_orphaned = OrphanWatchBackend::global_orphan_pool().pop();
    // SAFETY: the orphan's mapping is never freed while watched.
    let live_blocks = unsafe { (*orphan).pages[1].alloc_count };
    if let Some(parked) = still_orphaned {
        // SAFETY: popped exclusively; returned to the pool for `drain_pools`.
        unsafe { OrphanWatchBackend::global_orphan_pool().push_unbounded(parked) };
    }
    drain_pools();

    assert!(
        !handed_out_orphan,
        "allocate_segment handed out an orphan holding {ORPHAN_LIVE_BLOCKS} live blocks as an empty segment"
    );
    assert_eq!(
        releases, 0,
        "the live orphan's mapping was released to the backend"
    );
    assert_eq!(
        still_orphaned,
        Some(orphan),
        "the live orphan must stay parked in the orphan pool"
    );
    assert_eq!(
        live_blocks, ORPHAN_LIVE_BLOCKS,
        "the orphan's live-block count must survive the round trip"
    );
}

#[test]
fn acquire_segment_prefers_free_then_orphan_then_fresh_mapping() {
    let _guard = ORPHAN_WATCH_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    drain_pools();
    let orphan = park_orphan_with_live_blocks();
    // SAFETY: the segment is released exactly once below.
    let free = unsafe { allocate_segment::<OrphanWatchBackend>() }
        .expect("heap-backed segment mapping must succeed");
    // Retained directly rather than through `deallocate_segment`, whose
    // retention cap may decline it and release the mapping instead.
    // SAFETY: `free` is exclusively owned and empty.
    unsafe { OrphanWatchBackend::global_segment_pool().push_unbounded(free) };

    // SAFETY: each acquired segment is returned to its pool below.
    let (first, second, third) = unsafe {
        (
            acquire_segment::<OrphanWatchBackend>(),
            acquire_segment::<OrphanWatchBackend>(),
            acquire_segment::<OrphanWatchBackend>(),
        )
    };
    // SAFETY: the orphan's mapping is never freed while watched.
    let live_blocks = unsafe { (*orphan).pages[1].alloc_count };
    // SAFETY: every acquired segment is exclusively owned; the orphan goes
    // back to the orphan pool, the empty segments to the free pool.
    unsafe {
        for acquired in [first, second, third].into_iter().flatten() {
            match acquired {
                AcquiredSegment::Orphan(segment) => {
                    OrphanWatchBackend::global_orphan_pool().push_unbounded(segment);
                }
                AcquiredSegment::Free(segment) => {
                    deallocate_segment::<OrphanWatchBackend>(segment);
                }
            }
        }
    }
    drain_pools();

    assert_eq!(first, Some(AcquiredSegment::Free(free)));
    assert_eq!(second, Some(AcquiredSegment::Orphan(orphan)));
    assert!(
        matches!(third, Some(AcquiredSegment::Free(fresh)) if fresh != free && fresh != orphan),
        "with both pools empty the third acquisition must map a fresh segment, got {third:?}"
    );
    assert_eq!(
        live_blocks, ORPHAN_LIVE_BLOCKS,
        "an acquired orphan is handed out as is, live blocks intact"
    );
}
