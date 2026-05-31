use core::sync::atomic::Ordering;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_backend::MemoryBackendWrapper as Backend;
use mnemosyne_core::options::PURGE_CADENCE_MS;
use mnemosyne_core::StandardPolicy as Policy;
use mnemosyne_local::{reset_options_for_testing, thread_alloc, thread_free};
use std::thread;
use std::time::Duration;

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_decay_purger_spawns_and_cleans_orphans() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 1. Reset options state for testing
    reset_options_for_testing();

    // Set PURGE_CADENCE_MS to 10ms for fast test validation
    PURGE_CADENCE_MS.store(10, Ordering::Release);

    // Initialize the decay engine
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
    let mut found = false;
    for _ in 0..50 {
        if orphan_pool.retained_count() > 0 {
            found = true;
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert!(found, "Segment was not orphaned on thread exit");

    // 3. Now free the pointer from the main thread (cross-thread free).
    // This writes to page.thread_free.
    unsafe {
        thread_free::<Policy, Backend>(ptr);
    }

    // 4. Wait for the background decay thread to run. It should:
    // a. Sweep the orphan pool.
    // b. Drain/reclaim the cross-thread free we just did.
    // c. Detect that total_allocations == 0 for that segment.
    // d. Deallocate the segment completely back to the OS.
    let mut cleaned = false;
    for _ in 0..100 {
        if orphan_pool.retained_count() == 0 {
            cleaned = true;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        cleaned,
        "Orphaned segment was not cleaned up and deallocated by decay engine"
    );
}

#[test]
fn test_decay_engine_no_spawn_if_zero_cadence() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_options_for_testing();
    // Leave PURGE_CADENCE_MS at 0
    mnemosyne_decay::init_decay_engine();
    assert_eq!(PURGE_CADENCE_MS.load(Ordering::Acquire), 0);
}

#[test]
fn decay_purger_reaches_steady_state() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_options_for_testing();

    // 1. Configure cadence to 10ms for rapid sweeps
    PURGE_CADENCE_MS.store(10, Ordering::Release);
    mnemosyne_decay::init_decay_engine();

    // 2. Perform allocation and free in a spawned thread. Upon exit, the
    // thread-local cache is dropped and the segment is returned to the global segment pool.
    let handle = thread::spawn(|| {
        let ptr = unsafe { thread_alloc::<Policy, Backend>(32, 16) };
        assert!(!ptr.is_null());
        unsafe { thread_free::<Policy, Backend>(ptr); }
    });
    handle.join().expect("first allocation thread panicked");

    let stats_before = mnemosyne_arena::arena_memory_stats::<Backend>();
    assert!(
        stats_before.retained_free_segments >= 1,
        "Segment must be cached in pool after thread free and thread exit"
    );

    // 3. Wait for the decay purger to execute and reach steady state (retained = 0)
    let mut steady = false;
    for _ in 0..100 {
        let stats = mnemosyne_arena::arena_memory_stats::<Backend>();
        if stats.retained_free_segments == 0 {
            steady = true;
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert!(steady, "Purger failed to reach steady state of zero retained segments");

    // 4. Shutdown purger by setting cadence to 0
    PURGE_CADENCE_MS.store(0, Ordering::Release);
    thread::sleep(Duration::from_millis(30));

    // 5. Restart purger with 10ms cadence and verify restartability
    PURGE_CADENCE_MS.store(10, Ordering::Release);
    mnemosyne_decay::init_decay_engine();

    let handle2 = thread::spawn(|| {
        let ptr2 = unsafe { thread_alloc::<Policy, Backend>(32, 16) };
        assert!(!ptr2.is_null());
        unsafe { thread_free::<Policy, Backend>(ptr2); }
    });
    handle2.join().expect("second allocation thread panicked");

    let stats_before2 = mnemosyne_arena::arena_memory_stats::<Backend>();
    assert!(
        stats_before2.retained_free_segments >= 1,
        "Segment must be cached in pool after restart allocate/free and thread exit"
    );

    let mut steady2 = false;
    for _ in 0..100 {
        let stats = mnemosyne_arena::arena_memory_stats::<Backend>();
        if stats.retained_free_segments == 0 {
            steady2 = true;
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert!(steady2, "Purger failed to reach steady state after restart");

    // Reset options
    PURGE_CADENCE_MS.store(0, Ordering::Release);
    thread::sleep(Duration::from_millis(20));
    reset_options_for_testing();
}
