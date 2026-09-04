use core::sync::atomic::Ordering;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_backend::MemoryBackendWrapper as Backend;
use mnemosyne_core::StandardPolicy as Policy;
use mnemosyne_core::options::PURGE_CADENCE_MS;
use mnemosyne_local::internal::reset_options_for_testing;
use mnemosyne_local::{thread_alloc, thread_allocator_stats, thread_free};
use std::thread;
use std::time::Duration;

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(not(miri))]
const BACKGROUND_DECAY_TIMEOUT: Duration = Duration::from_secs(5);
// The wait is event-based, but Miri interprets every pointer operation and
// runs this binary concurrently with other interpreters. The native timeout
// is sufficient for a real worker; the Miri-specific bound covers the measured
// interpreter cost without changing the native test budget.
#[cfg(miri)]
const BACKGROUND_DECAY_TIMEOUT: Duration = Duration::from_secs(180);
const FRAGMENTATION_SIZES: [usize; 4] = [16, 64, 256, 1024];
const FRAGMENTATION_ROUNDS: usize = 64;
const TEMPORARY_BLOCKS_PER_CLASS: usize = 32;
const PINNED_LIVE_BYTES: usize = 16 + 64 + 256 + 1024;

const _: () = assert!(
    (TEMPORARY_BLOCKS_PER_CLASS + 1) * 1024 <= mnemosyne_core::constants::PAGE_SIZE,
    "the largest size-class wave must fit in one page"
);
const _: () = assert!(
    FRAGMENTATION_SIZES.len() < mnemosyne_core::constants::PAGES_PER_SEGMENT,
    "the active size classes must fit in one segment after its metadata page"
);

fn allocate_patterned_block(size: usize, pattern: u8) -> *mut u8 {
    // SAFETY: the requested size and alignment are non-zero, and the returned
    // block is retained until one matching `thread_free` call.
    let ptr = unsafe { thread_alloc::<Policy, Backend>(size, 16) };
    assert!(!ptr.is_null(), "allocation failed for {size}-byte block");
    // SAFETY: a successful allocation provides `size` writable bytes.
    unsafe { ptr.write_bytes(pattern, size) };
    ptr
}

fn free_block(ptr: *mut u8) {
    // SAFETY: callers pass each pointer returned by
    // `allocate_patterned_block` exactly once.
    unsafe { thread_free::<Policy, Backend>(ptr) };
}

#[test]
fn test_decay_purger_spawns_and_cleans_orphans() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 1. Reset options state for testing
    reset_options_for_testing();

    // Keep the worker disabled while the orphan workload is prepared.
    PURGE_CADENCE_MS.store(0, Ordering::Release);
    mnemosyne_decay::init_decay_engine();

    // 2. Spawn a thread, perform an allocation to claim a segment, and let it exit to orphan it.
    let handle = thread::spawn(|| {
        let ptr = unsafe { thread_alloc::<Policy, Backend>(32, 16) };
        assert!(!ptr.is_null());
        ptr as usize
    });

    let ptr_val = handle.join().expect("spawned thread panicked");
    let ptr = ptr_val as *mut u8;

    // The segment should now be owned by the orphan pool because the allocating thread exited
    // with a live allocation. Let's verify that the orphan pool contains at least 1 segment.
    let orphan_pool = <Backend as HasSegmentPool>::global_orphan_pool();
    assert!(
        orphan_pool.retained_count() > 0,
        "Segment was not orphaned on thread exit"
    );

    // 3. Now free the pointer from the main thread (cross-thread free).
    // This writes to page.thread_free.
    let generation_before_free = mnemosyne_decay::decay_step_generation();
    unsafe {
        thread_free::<Policy, Backend>(ptr);
    }

    // 4. Start the background worker after the zero-allocation condition is
    // established. It should:
    // a. Sweep the orphan pool.
    // b. Drain/reclaim the cross-thread free we just did.
    // c. Detect that total_allocations == 0 for that segment.
    // d. Deallocate the segment completely back to the OS.
    PURGE_CADENCE_MS.store(10, Ordering::Release);
    mnemosyne_decay::init_decay_engine();
    mnemosyne_decay::request_decay_step();
    assert!(
        mnemosyne_decay::wait_for_decay_step(generation_before_free, BACKGROUND_DECAY_TIMEOUT),
        "background decay did not complete a sweep"
    );
    assert_eq!(
        orphan_pool.retained_count(),
        0,
        "Orphaned segment was not cleaned up and deallocated by decay engine"
    );

    PURGE_CADENCE_MS.store(0, Ordering::Release);
    mnemosyne_decay::init_decay_engine();
    assert!(
        mnemosyne_decay::wait_for_decay_shutdown(BACKGROUND_DECAY_TIMEOUT),
        "background decay worker did not shut down"
    );
    reset_options_for_testing();
}

#[test]
fn test_decay_engine_no_spawn_if_zero_cadence() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_options_for_testing();
    // Leave PURGE_CADENCE_MS at 0
    mnemosyne_decay::init_decay_engine();
    assert_eq!(PURGE_CADENCE_MS.load(Ordering::Acquire), 0);
    assert!(mnemosyne_decay::wait_for_decay_shutdown(
        BACKGROUND_DECAY_TIMEOUT
    ));
}

#[test]
fn decay_shutdown_timeout_does_not_report_running_worker_as_stopped() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_options_for_testing();

    // Keep the worker asleep long enough that a zero-duration wait observes
    // the active worker rather than its eventual final-exit publication.
    PURGE_CADENCE_MS.store(1_000, Ordering::Release);
    mnemosyne_decay::init_decay_engine();
    assert!(
        !mnemosyne_decay::wait_for_decay_shutdown(Duration::ZERO),
        "shutdown wait must report its timeout while the worker is active"
    );

    PURGE_CADENCE_MS.store(0, Ordering::Release);
    mnemosyne_decay::init_decay_engine();
    assert!(mnemosyne_decay::wait_for_decay_shutdown(
        BACKGROUND_DECAY_TIMEOUT
    ));
    reset_options_for_testing();
}

#[test]
fn decay_purger_concurrent_restart_preserves_value_progress() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_options_for_testing();

    PURGE_CADENCE_MS.store(1, Ordering::Release);
    mnemosyne_decay::init_decay_engine();
    mnemosyne_decay::request_decay_step();
    let generation_before_restart = mnemosyne_decay::decay_step_generation();

    let restart = thread::spawn(|| {
        for _ in 0..32 {
            PURGE_CADENCE_MS.store(1, Ordering::Release);
            mnemosyne_decay::init_decay_engine();
            thread::yield_now();
            PURGE_CADENCE_MS.store(0, Ordering::Release);
        }
    });
    restart.join().expect("concurrent decay restart panicked");

    PURGE_CADENCE_MS.store(1, Ordering::Release);
    mnemosyne_decay::init_decay_engine();
    mnemosyne_decay::request_decay_step();
    assert!(
        mnemosyne_decay::wait_for_decay_step(generation_before_restart, BACKGROUND_DECAY_TIMEOUT),
        "a concurrent restart must preserve completed decay progress"
    );

    PURGE_CADENCE_MS.store(0, Ordering::Release);
    mnemosyne_decay::init_decay_engine();
    assert!(mnemosyne_decay::wait_for_decay_shutdown(
        BACKGROUND_DECAY_TIMEOUT
    ));
    reset_options_for_testing();
}

#[test]
fn decay_purger_reaches_steady_state() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_options_for_testing();

    // 1. Keep the worker disabled while the retained-segment workload is
    // prepared.
    PURGE_CADENCE_MS.store(0, Ordering::Release);
    mnemosyne_decay::init_decay_engine();

    // 2. Perform allocation and free in a spawned thread. Upon exit, the
    // thread-local cache is dropped and the segment is returned to the global segment pool.
    let generation_before_workload = mnemosyne_decay::decay_step_generation();
    let handle = thread::spawn(|| {
        let ptr = unsafe { thread_alloc::<Policy, Backend>(32, 16) };
        assert!(!ptr.is_null());
        unsafe {
            thread_free::<Policy, Backend>(ptr);
        }
    });
    handle.join().expect("first allocation thread panicked");

    let stats_before = mnemosyne_arena::arena_memory_stats::<Backend>();
    assert!(
        stats_before.retained_free_segments >= 1,
        "Segment must be cached in pool after thread free and thread exit"
    );

    // 3. Start the worker after the retained segment exists and wait for the
    // corresponding completed sweep.
    PURGE_CADENCE_MS.store(10, Ordering::Release);
    mnemosyne_decay::init_decay_engine();
    mnemosyne_decay::request_decay_step();
    assert!(
        mnemosyne_decay::wait_for_decay_step(generation_before_workload, BACKGROUND_DECAY_TIMEOUT,),
        "background decay did not complete the first steady-state sweep"
    );
    assert_eq!(
        mnemosyne_arena::arena_memory_stats::<Backend>().retained_free_segments,
        0,
        "Purger failed to reach steady state of zero retained segments"
    );

    // 4. Shutdown purger by setting cadence to 0
    PURGE_CADENCE_MS.store(0, Ordering::Release);
    mnemosyne_decay::init_decay_engine();
    assert!(
        mnemosyne_decay::wait_for_decay_shutdown(BACKGROUND_DECAY_TIMEOUT),
        "background decay worker did not shut down"
    );

    // 5. Prepare a second retained-segment workload while the worker is
    // disabled, then restart it and verify restartability.
    let generation_before_restart = mnemosyne_decay::decay_step_generation();
    let handle2 = thread::spawn(|| {
        let ptr2 = unsafe { thread_alloc::<Policy, Backend>(32, 16) };
        assert!(!ptr2.is_null());
        unsafe {
            thread_free::<Policy, Backend>(ptr2);
        }
    });
    handle2.join().expect("second allocation thread panicked");

    let stats_before2 = mnemosyne_arena::arena_memory_stats::<Backend>();
    assert!(
        stats_before2.retained_free_segments >= 1,
        "Segment must be cached in pool after restart allocate/free and thread exit"
    );

    PURGE_CADENCE_MS.store(10, Ordering::Release);
    mnemosyne_decay::init_decay_engine();
    mnemosyne_decay::request_decay_step();
    assert!(
        mnemosyne_decay::wait_for_decay_step(generation_before_restart, BACKGROUND_DECAY_TIMEOUT),
        "background decay did not complete the restart sweep"
    );
    assert_eq!(
        mnemosyne_arena::arena_memory_stats::<Backend>().retained_free_segments,
        0,
        "Purger failed to reach steady state after restart"
    );

    // Reset options
    PURGE_CADENCE_MS.store(0, Ordering::Release);
    mnemosyne_decay::init_decay_engine();
    assert!(
        mnemosyne_decay::wait_for_decay_shutdown(BACKGROUND_DECAY_TIMEOUT),
        "background decay worker did not shut down after restart"
    );
    reset_options_for_testing();
}

/// Deterministic RSS-return verification: calls `decay_step()` directly
/// instead of polling the background thread, and verifies the byte-level
/// accounting matches the segment deallocation.
///
/// Exercises:
/// 1. Thread allocates → exits → segment enters orphan pool.
/// 2. Cross-thread free makes total_allocations == 0.
/// 3. `decay_step()` drains the orphan pool, detects zero allocations,
///    and calls `deallocate_segment` → segment mapping released to OS.
/// 4. `arena_memory_stats` confirms: retained_free_segments decreased,
///    purged_bytes increased.
#[test]
fn decay_step_returns_segment_bytes_to_os() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_options_for_testing();

    // Record baseline stats.
    let before = mnemosyne_arena::arena_memory_stats::<Backend>();

    // 1. Spawn a thread, allocate, and let it exit to orphan the segment.
    let handle = thread::spawn(|| {
        let ptr = unsafe { thread_alloc::<Policy, Backend>(32, 16) };
        assert!(!ptr.is_null(), "orphan producer alloc failed");
        ptr as usize
    });
    let ptr_val = handle.join().expect("orphan producer panicked");
    let ptr = ptr_val as *mut u8;

    let orphan_pool = <Backend as HasSegmentPool>::global_orphan_pool();
    assert!(
        orphan_pool.retained_count() > 0,
        "segment was not orphaned on thread exit"
    );

    // 2. Cross-thread free: reduces total_allocations to 0 for this segment.
    unsafe {
        thread_free::<Policy, Backend>(ptr);
    }

    // 3. Call decay_step() directly — deterministic, no polling.
    mnemosyne_decay::decay_step();

    // 4. Verify accounting: the orphan pool must be drained.
    let after_retained = orphan_pool.retained_count();
    assert_eq!(
        after_retained, 0,
        "decay_step must drain the orphan pool (retained={after_retained})"
    );

    // 5. Verify byte-level accounting changed.
    let after = mnemosyne_arena::arena_memory_stats::<Backend>();
    assert!(
        after.purged_bytes > before.purged_bytes,
        "purged_bytes must increase after decay_step: before={}, after={}",
        before.purged_bytes,
        after.purged_bytes
    );
}

#[test]
fn alternating_size_classes_converge_with_pinned_survivors() {
    struct ResetOptionsOnDrop;

    impl Drop for ResetOptionsOnDrop {
        fn drop(&mut self) {
            reset_options_for_testing();
        }
    }

    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    reset_options_for_testing();
    let _reset = ResetOptionsOnDrop;
    mnemosyne_core::options::MAX_RETAINED_SEGMENTS.store(0, Ordering::Release);
    mnemosyne_decay::decay_step();

    let baseline_mapped = mnemosyne_backend::backend_memory_stats().current_mapped_bytes;
    let worker = thread::spawn(move || {
        let patterns = [0x11, 0x33, 0x55, 0x77];
        let pinned: [(*mut u8, usize, u8); FRAGMENTATION_SIZES.len()] =
            core::array::from_fn(|index| {
                let size = FRAGMENTATION_SIZES[index];
                let pattern = patterns[index];
                (allocate_patterned_block(size, pattern), size, pattern)
            });

        for round in 0..FRAGMENTATION_ROUNDS {
            let sizes = if round.is_multiple_of(2) {
                FRAGMENTATION_SIZES
            } else {
                [1024, 256, 64, 16]
            };
            let mut temporary =
                Vec::with_capacity(FRAGMENTATION_SIZES.len() * TEMPORARY_BLOCKS_PER_CLASS);
            for size in sizes {
                temporary.extend(
                    (0..TEMPORARY_BLOCKS_PER_CLASS).map(|_| allocate_patterned_block(size, 0xA5)),
                );
            }

            temporary.into_iter().rev().for_each(free_block);

            let allocator = thread_allocator_stats::<Policy, Backend>();
            assert_eq!(
                allocator.current_thread_live_allocations,
                FRAGMENTATION_SIZES.len(),
                "round {round} must retain only the four pinned survivors"
            );
            assert_eq!(
                allocator.fresh_segments, 1,
                "round {round} mapped another segment"
            );
            assert_eq!(
                allocator.current_thread_owned_segments, 1,
                "round {round} escaped the one-segment working-set bound"
            );

            let mapped = mnemosyne_backend::backend_memory_stats().current_mapped_bytes;
            let mapped_delta = mapped
                .checked_sub(baseline_mapped)
                .expect("invariant: this workload cannot unmap the pre-test baseline");
            assert!(
                mapped_delta <= mnemosyne_arena::SEGMENT_MAPPING_SIZE,
                "round {round} retains {mapped_delta} mapped bytes for {PINNED_LIVE_BYTES} pinned bytes; bound is one {}-byte segment mapping",
                mnemosyne_arena::SEGMENT_MAPPING_SIZE
            );

            for &(ptr, size, pattern) in &pinned {
                // SAFETY: pinned blocks remain allocated throughout every
                // round, and both reads stay inside their allocation.
                unsafe {
                    assert_eq!(ptr.read(), pattern);
                    assert_eq!(ptr.add(size - 1).read(), pattern);
                }
            }
        }

        let allocator = thread_allocator_stats::<Policy, Backend>();
        assert_eq!(
            allocator.current_thread_live_allocations,
            FRAGMENTATION_SIZES.len(),
            "only the four pinned survivors may remain live"
        );
        for &(ptr, _, _) in &pinned {
            free_block(ptr);
        }
    });
    worker.join().expect("fragmentation worker panicked");

    mnemosyne_decay::decay_step();
    assert_eq!(
        mnemosyne_backend::backend_memory_stats().current_mapped_bytes,
        baseline_mapped,
        "the completed workload must return to its pre-test mapping baseline"
    );
}
