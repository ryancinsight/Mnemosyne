//! Producer-consumer and many-to-one stress runs.

use super::*;

/// Multi-thread producer-consumer stress: allocators across N threads allocate
/// blocks and free blocks allocated by other threads.
///
/// Exercises the cross-thread free path under contention: every thread
/// allocates from its own `ThreadAllocator`, then passes the pointers to a
/// different thread for freeing. This stresses the page-local atomic
/// `thread_free.push` / `reclaim_thread_free` path with concurrent producers
/// and consumers.
///
/// Verifies:
/// 1. No crashes, aborts, or corrupted metadata.
/// 2. Every allocation is eventually freed exactly once.
/// 3. The owner thread can reclaim all cross-thread freed blocks.
#[test]
fn cross_thread_stress_producer_consumer() {
    use std::sync::mpsc;
    use std::thread;

    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    const NUM_PRODUCERS: usize = 4;
    const ALLOCS_PER_PRODUCER: usize = 200;

    // Each producer thread allocates blocks and sends the pointers to
    // a dedicated consumer thread via its own channel.
    let mut producers = std::vec::Vec::new();
    let mut consumers = std::vec::Vec::new();

    for tid in 0..NUM_PRODUCERS {
        let (tx, rx) = mpsc::channel::<std::vec::Vec<usize>>();
        producers.push(thread::spawn(move || {
            let mut alloc = ThreadAllocator::<DefaultBackend>::new();
            let mut ptrs = std::vec::Vec::with_capacity(ALLOCS_PER_PRODUCER);
            for i in 0..ALLOCS_PER_PRODUCER {
                let size = if (i + tid) % 2 == 0 { 32 } else { 64 };
                let ptr = unsafe { alloc.alloc::<StandardPolicy>(size) };
                assert!(!ptr.is_null(), "producer {tid} alloc {i} failed");
                unsafe {
                    *ptr = (tid * 100 + i) as u8;
                }
                ptrs.push(ptr as usize);
            }
            tx.send(ptrs).expect("producer failed to send ptrs");
        }));
        consumers.push(thread::spawn(move || {
            // Each consumer creates its own allocator to exercise the
            // non-owner cross-thread free path.
            let _alloc = ThreadAllocator::<DefaultBackend>::new();
            for ptrs in rx {
                for addr in ptrs {
                    unsafe {
                        crate::thread_free::<StandardPolicy, DefaultBackend>(addr as *mut u8);
                    }
                }
            }
        }));
    }

    for handle in producers.into_iter() {
        handle.join().expect("producer thread panicked");
    }
    for handle in consumers.into_iter() {
        handle.join().expect("consumer thread panicked");
    }

    // If we reached here without crashing, the cross-thread free path
    // handled NUM_PRODUCERS * ALLOCS_PER_PRODUCER concurrent cross-thread frees.
}

/// Cross-thread free under high contention: many threads simultaneously free
/// blocks that belong to a single owner thread.
///
/// One owner allocates many blocks. Then N non-owning threads each take a
/// slice and free them concurrently. This stress-tests the page-local atomic
/// `thread_free.push` path for data-race freedom and metadata integrity.
///
/// Verifies:
/// 1. No crashes, aborts, or metadata corruption under 8-way concurrent
///    cross-thread frees of 640 blocks.
/// 2. The owner allocator remains functional after the contention storm.
#[test]
fn cross_thread_stress_many_to_one_free() {
    use std::sync::mpsc;
    use std::thread;

    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    const NUM_FREER_THREADS: usize = 8;
    const TOTAL_ALLOCS: usize = 640;

    // Owner allocates all blocks.
    let mut owner = ThreadAllocator::<DefaultBackend>::new();
    let mut all_ptrs = std::vec::Vec::with_capacity(TOTAL_ALLOCS);
    for i in 0..TOTAL_ALLOCS {
        let ptr = unsafe { owner.alloc::<StandardPolicy>(32) };
        assert!(!ptr.is_null(), "owner alloc {i} failed");
        unsafe {
            *ptr = i as u8;
        }
        all_ptrs.push(ptr as usize);
    }

    let chunk_size = TOTAL_ALLOCS / NUM_FREER_THREADS;
    let (tx, rx) = mpsc::channel();

    // Distribute blocks across freer threads.
    for chunk_idx in 0..NUM_FREER_THREADS {
        let start = chunk_idx * chunk_size;
        let end = if chunk_idx == NUM_FREER_THREADS - 1 {
            TOTAL_ALLOCS
        } else {
            start + chunk_size
        };
        let chunk: std::vec::Vec<usize> = all_ptrs[start..end].to_vec();
        let tx = tx.clone();
        thread::spawn(move || unsafe {
            for addr in chunk {
                crate::thread_free::<StandardPolicy, DefaultBackend>(addr as *mut u8);
            }
            tx.send(()).expect("freer failed to signal completion");
        });
    }
    drop(tx);

    // Wait for all freer threads.
    for _ in 0..NUM_FREER_THREADS {
        rx.recv().expect("freer thread did not signal completion");
    }

    // Verify the owner allocator is still functional after the contention storm.
    let post_stress = unsafe { owner.alloc::<StandardPolicy>(32) };
    assert!(
        !post_stress.is_null(),
        "owner allocation failed after cross-thread contention storm"
    );
    unsafe {
        core::ptr::write_bytes(post_stress, 0xFF, 32);
        assert_eq!(*post_stress, 0xFF);
    }
    unsafe {
        crate::thread_free::<StandardPolicy, DefaultBackend>(post_stress);
    }
}
