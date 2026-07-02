use super::super::*;
use super::fixtures::{ALLOC_COUNT, DEALLOC_COUNT, MockBackend};
use core::sync::atomic::Ordering;
use mnemosyne_core::policy::StandardPolicy;

#[test]
fn test_custom_backend_injection() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    ALLOC_COUNT.store(0, Ordering::SeqCst);
    DEALLOC_COUNT.store(0, Ordering::SeqCst);

    // Verify that the code compiles with ThreadAllocator parameterized by MockBackend
    let mut alloc = ThreadAllocator::<MockBackend>::new();
    // Safety: alloc is initialized and valid.
    let ptr = unsafe { alloc.alloc::<StandardPolicy>(32) };
    assert!(!ptr.is_null(), "MockBackend small allocation failed");
    unsafe {
        crate::thread_free::<mnemosyne_core::StandardPolicy, MockBackend>(ptr);
    }

    // Verify large allocation directly calls MockBackend
    // Safety: size and align are valid.
    let large_ptr =
        unsafe { mnemosyne_arena::allocate_large_or_huge::<MockBackend>(1024 * 1024, 8, true) };
    assert!(!large_ptr.is_null(), "MockBackend large allocation failed");
    assert!(
        ALLOC_COUNT.load(Ordering::SeqCst) >= 1,
        "MockBackend allocate counter was {}",
        ALLOC_COUNT.load(Ordering::SeqCst)
    );

    // Safety: large_ptr points to huge allocation segment.
    unsafe {
        let seg =
            ((large_ptr as usize) & !(mnemosyne_core::constants::SEGMENT_SIZE - 1)) as *mut Segment;
        let _released = mnemosyne_arena::deallocate_large_or_huge::<MockBackend>(large_ptr, seg);
        mnemosyne_arena::segment::purge_segment_pool::<MockBackend>();
    }
    assert!(
        DEALLOC_COUNT.load(Ordering::SeqCst) >= 1,
        "MockBackend deallocate counter was {}",
        DEALLOC_COUNT.load(Ordering::SeqCst)
    );
}

/// Proves the `#[thread_local]` fast cache still reclaims a terminating
/// thread's owned segments. A `#[thread_local]` static is not dropped on
/// thread exit, so reclamation depends entirely on the exit sentinel
/// (`ThreadExitReclaim`) armed on first allocation. The spawned thread
/// allocates through the TLS path and exits without freeing; the live
/// segment must therefore be orphaned into `MockBackend`'s orphan pool. If the
/// sentinel failed to fire the segment would leak and the pool would stay
/// empty, failing the value-semantic assertion below.
#[cfg(nightly_tls_active)]
#[test]
fn thread_exit_sentinel_reclaims_owned_segments_on_fast_tls_path() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    // Drain any residue so the post-join count reflects only this thread.
    while let Some(seg) =
        <MockBackend as mnemosyne_arena::HasSegmentPool>::global_orphan_pool().pop()
    {
        // Safety: pooled segments are valid mappings owned by the pool.
        unsafe { mnemosyne_arena::deallocate_segment::<MockBackend>(seg) };
    }

    let handle = std::thread::spawn(|| {
        // Safety: a 32-byte/16-align request is a valid small allocation;
        // routing through the TLS path arms the exit sentinel and acquires
        // a segment owned by this thread. The block is intentionally not
        // freed so the owning segment is still live at thread exit.
        let ptr =
            unsafe { crate::thread_alloc::<mnemosyne_core::StandardPolicy, MockBackend>(32, 16) };
        assert!(!ptr.is_null(), "fast-TLS small allocation failed");
        ptr as usize
    });
    let block_addr = handle.join().expect("spawned allocator thread panicked");
    assert_ne!(block_addr, 0, "spawned thread produced a null allocation");

    // The exit sentinel must have orphaned the still-live owning segment.
    let mut reclaimed = 0usize;
    while let Some(seg) =
        <MockBackend as mnemosyne_arena::HasSegmentPool>::global_orphan_pool().pop()
    {
        reclaimed += 1;
        // Safety: pooled segments are valid mappings; release the mapping
        // (including the never-freed block) to avoid leaking the test's
        // owned segment beyond the assertion.
        unsafe { mnemosyne_arena::deallocate_segment::<MockBackend>(seg) };
    }
    assert!(
        reclaimed >= 1,
        "thread-exit sentinel did not reclaim the live owned segment; \
         orphan pool received {reclaimed} segments"
    );
}

#[test]
fn thread_exit_reclaims_owned_segments_on_selected_tls_path() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    // Drain any residue so the post-join count reflects only this thread.
    while let Some(seg) =
        <MockBackend as mnemosyne_arena::HasSegmentPool>::global_orphan_pool().pop()
    {
        // Safety: pooled segments are valid mappings owned by the pool.
        unsafe { mnemosyne_arena::deallocate_segment::<MockBackend>(seg) };
    }

    let handle = std::thread::spawn(|| {
        // Safety: a 32-byte/16-align request is a valid small allocation;
        // routing through the TLS path arms the exit sentinel (or registers standard drop)
        // and acquires a segment owned by this thread. The block is intentionally not
        // freed so the owning segment is still live at thread exit.
        let ptr =
            unsafe { crate::thread_alloc::<mnemosyne_core::StandardPolicy, MockBackend>(32, 16) };
        assert!(!ptr.is_null(), "selected-TLS small allocation failed");
        ptr as usize
    });
    let block_addr = handle.join().expect("spawned allocator thread panicked");
    assert_ne!(block_addr, 0, "spawned thread produced a null allocation");

    // The exit sentinel or slot drop must have orphaned the still-live owning segment.
    let mut reclaimed = 0usize;
    while let Some(seg) =
        <MockBackend as mnemosyne_arena::HasSegmentPool>::global_orphan_pool().pop()
    {
        reclaimed += 1;
        // Safety: pooled segments are valid mappings; release the mapping
        // (including the never-freed block) to avoid leaking the test's
        // owned segment beyond the assertion.
        unsafe { mnemosyne_arena::deallocate_segment::<MockBackend>(seg) };
    }
    assert!(
        reclaimed >= 1,
        "thread-exit did not reclaim the live owned segment; \
         orphan pool received {reclaimed} segments"
    );
}
