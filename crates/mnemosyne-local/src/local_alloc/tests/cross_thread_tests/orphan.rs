//! Orphaned-segment reuse and policy-aware adoption.

use super::*;

#[test]
fn test_orphan_segment_reuse() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use std::sync::mpsc;
    use std::thread;

    unsafe {
        mnemosyne_arena::purge_segment_pool::<DefaultBackend>();
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }

    let (tx, rx) = mpsc::channel();

    // Thread A allocates a block and exits
    thread::spawn(move || {
        let mut alloc_a = ThreadAllocator::<DefaultBackend>::new();
        // SAFETY: alloc_a is valid.
        let ptr = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
        assert!(!ptr.is_null(), "orphan producer allocation failed");
        tx.send(ptr as usize)
            .expect("orphan producer failed to send live allocation pointer");
    })
    .join()
    .expect("orphan producer thread panicked");

    let live_ptr =
        rx.recv()
            .expect("orphan producer did not send live allocation pointer") as *mut u8;

    // Thread B allocates a block. It should reuse the orphaned segment from A!
    let mut alloc_b = ThreadAllocator::<DefaultBackend>::new();
    // SAFETY: alloc_b is valid.
    let ptr_b = unsafe { alloc_b.alloc::<StandardPolicy>(64) };
    assert!(!ptr_b.is_null(), "orphan consumer allocation failed");

    // Assert that B reused the orphaned segment: current owned segments must be 1, not 2!
    assert_eq!(alloc_b.stats().current_thread_owned_segments, 1);

    // Free the allocations
    // SAFETY: pointers are valid and exclusive.
    unsafe {
        crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(live_ptr);
        crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(ptr_b);
    }
}

/// Drains the orphan pools left behind by other tests so orphan-adoption tests
/// observe a deterministic pool state, releasing each drained segment through
/// the regular deallocation path.
///
/// # Safety
///
/// Callers must hold `TEST_LOCK` so no concurrent allocator activity races the
#[test]
fn test_hardened_orphan_adoption_preserves_encoded_chains() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    // The consumer has to exhaust the page adoption hands it before it reaches
    // the producer's encoded chain, so the block size sets the cost of the
    // whole test: a page holds `PAGE_SIZE / BLOCK` blocks. The property here —
    // that adoption preserves the producer's per-page keys, so the encoded
    // `page.free` chain still decodes — is the same in every small class, so
    // this uses a large one. At 32 bytes the sweep was 2048 allocations per
    // page and the test could not finish inside the Miri budget; at 2048 it is
    // 32, with identical coverage.
    const BLOCK: usize = 2048;
    use mnemosyne_core::policy::HardenedPolicy;
    use std::sync::mpsc;
    use std::thread;
    use std::vec::Vec;

    // SAFETY: TEST_LOCK is held; no concurrent allocator activity.
    unsafe { super::super::fixtures::drain_all_pools() };

    let (tx, rx) = mpsc::channel();

    // Producer: allocate four blocks under the encrypted policy, free two of
    // them (building a `page.free` chain encoded with THIS thread's per-page
    // keys), keep two live, and exit so the segment is orphaned with a live
    // encoded chain.
    thread::spawn(move || {
        let mut alloc_a = ThreadAllocator::<DefaultBackend>::new();
        let ptrs: Vec<*mut u8> = (0..4)
            // SAFETY: alloc_a is valid; BLOCK is a small size class.
            .map(|_| unsafe { alloc_a.alloc::<HardenedPolicy>(BLOCK) })
            .collect();
        assert!(
            ptrs.iter().all(|p| !p.is_null()),
            "hardened orphan producer allocation failed"
        );
        // SAFETY: freeing two distinct pointers just allocated on this thread.
        unsafe {
            crate::thread_free::<HardenedPolicy, DefaultBackend>(ptrs[1]);
            crate::thread_free::<HardenedPolicy, DefaultBackend>(ptrs[3]);
        }
        tx.send((
            [ptrs[0] as usize, ptrs[2] as usize],
            [ptrs[1] as usize, ptrs[3] as usize],
        ))
        .expect("hardened orphan producer failed to send pointers");
    })
    .join()
    .expect("hardened orphan producer thread panicked");

    let (live, freed) = rx
        .recv()
        .expect("hardened orphan producer did not send pointers");

    // Consumer: a different thread (hence a different TLS key seed) adopts the
    // orphan. Before the key-preservation fix, adoption re-keyed the segment
    // with this thread's seed, so popping the producer-encoded `page.free`
    // chain decoded garbage and aborted on the free-list bounds check.
    let mut alloc_b = ThreadAllocator::<DefaultBackend>::new();
    // SAFETY: alloc_b is valid; BLOCK is a small size class.
    let first = unsafe { alloc_b.alloc::<HardenedPolicy>(BLOCK) };
    assert!(
        !first.is_null(),
        "hardened orphan consumer allocation failed"
    );
    let stats = alloc_b.stats();
    assert_eq!(
        stats.current_thread_owned_segments, 1,
        "consumer must adopt the compatible hardened orphan, not map a fresh segment"
    );
    assert_eq!(stats.orphan_segments_adopted, 1);

    // Allocate until the adopted page's producer-encoded free chain is popped:
    // the freshly initialized page the adoption returned holds
    // PAGE_SIZE / BLOCK blocks, after which the producer's active page (whose
    // `free` chain carries the two freed blocks) becomes the allocation
    // source. Reusing one of the freed addresses is the value-semantic proof
    // that the preserved keys decode the chain correctly.
    // Both freed blocks must come back: the first pop returns the chain head
    // and stores its decoded next-link as the new `page.free`; only the
    // second pop dereference-validates that decoded link, so requiring both
    // addresses is what proves the chain decodes correctly end-to-end (under
    // the re-keying bug the second pop aborts on the bounds check or yields a
    // garbage address outside the freed set).
    let cap = 3 * (mnemosyne_core::constants::PAGE_SIZE / BLOCK);
    let mut reused = 0usize;
    let mut consumer_ptrs = Vec::with_capacity(cap + 1);
    consumer_ptrs.push(first);
    for _ in 0..cap {
        // SAFETY: alloc_b is valid; BLOCK is a small size class.
        let p = unsafe { alloc_b.alloc::<HardenedPolicy>(BLOCK) };
        assert!(
            !p.is_null(),
            "hardened consumer allocation failed mid-sweep"
        );
        consumer_ptrs.push(p);
        if freed.contains(&(p as usize)) {
            // SAFETY: `p` was just returned by the allocator; 32 bytes are
            // writable block payload.
            unsafe {
                core::ptr::write_bytes(p, 0xAB, BLOCK);
                assert_eq!(*p, 0xAB);
                assert_eq!(*p.add(BLOCK - 1), 0xAB);
            }
            reused += 1;
            if reused == freed.len() {
                break;
            }
        }
    }
    assert_eq!(
        reused,
        freed.len(),
        "adopted encoded free chain was not fully popped within {cap} allocations"
    );

    // SAFETY: every pointer below was returned by this allocator family and is
    // freed exactly once (producer's live pair plus the consumer sweep).
    unsafe {
        for p in consumer_ptrs {
            crate::thread_free::<HardenedPolicy, DefaultBackend>(p);
        }
        for addr in live {
            crate::thread_free::<HardenedPolicy, DefaultBackend>(addr as *mut u8);
        }
    }
}

#[test]
fn test_orphan_adoption_skips_policy_mismatched_segment() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use mnemosyne_core::policy::HardenedPolicy;
    use std::sync::mpsc;
    use std::thread;

    // SAFETY: TEST_LOCK is held; no concurrent allocator activity.
    unsafe { super::super::fixtures::drain_all_pools() };

    let (tx, rx) = mpsc::channel();

    // Producer: orphan a plain (unencrypted) segment with one live block.
    thread::spawn(move || {
        let mut alloc_a = ThreadAllocator::<DefaultBackend>::new();
        // SAFETY: alloc_a is valid; 32 is a small size class.
        let ptr = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
        assert!(!ptr.is_null(), "standard orphan producer allocation failed");
        tx.send(ptr as usize)
            .expect("standard orphan producer failed to send pointer");
    })
    .join()
    .expect("standard orphan producer thread panicked");

    let live_ptr = rx
        .recv()
        .expect("standard orphan producer did not send pointer") as *mut u8;

    // An encrypted-policy consumer must NOT adopt the plain orphan: its free
    // chains are encoded with cookie 0 while `pop_block::<HardenedPolicy>`
    // would decode them with the per-page keys. The gate defers the orphan
    // back to the pool and takes a fresh segment instead.
    let mut alloc_hardened = ThreadAllocator::<DefaultBackend>::new();
    // SAFETY: allocator is valid; 32 is a small size class.
    let ptr_h = unsafe { alloc_hardened.alloc::<HardenedPolicy>(32) };
    assert!(!ptr_h.is_null(), "hardened consumer allocation failed");
    let stats_h = alloc_hardened.stats();
    assert_eq!(
        stats_h.orphan_segments_adopted, 0,
        "hardened consumer must not adopt a plain-encoded orphan"
    );
    assert_eq!(stats_h.fresh_segments, 1);
    assert_eq!(stats_h.current_thread_owned_segments, 1);

    // A matching-policy consumer still finds the deferred orphan in the pool.
    let mut alloc_standard = ThreadAllocator::<DefaultBackend>::new();
    // SAFETY: allocator is valid; 64 is a small size class.
    let ptr_s = unsafe { alloc_standard.alloc::<StandardPolicy>(64) };
    assert!(!ptr_s.is_null(), "standard consumer allocation failed");
    let stats_s = alloc_standard.stats();
    assert_eq!(
        stats_s.orphan_segments_adopted, 1,
        "standard consumer must adopt the deferred plain orphan"
    );
    assert_eq!(stats_s.current_thread_owned_segments, 1);

    // SAFETY: pointers are valid, freed once, under their allocation policies.
    unsafe {
        crate::thread_free::<StandardPolicy, DefaultBackend>(live_ptr);
        crate::thread_free::<HardenedPolicy, DefaultBackend>(ptr_h);
        crate::thread_free::<StandardPolicy, DefaultBackend>(ptr_s);
    }
}
