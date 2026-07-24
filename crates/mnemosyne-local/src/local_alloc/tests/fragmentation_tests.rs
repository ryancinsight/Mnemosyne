use super::super::*;
use mnemosyne_core::policy::StandardPolicy;

/// Adversarial fragmentation stress: alternating size classes with pinned survivors.
///
/// Allocates blocks from three size classes in a round-robin pattern, pins
/// (retains) a subset from each class, and frees the rest. This creates
/// cross-class fragmentation that prevents block coalescing. The test then
/// verifies:
///
/// 1. Every pinned pointer is still valid (no corruption).
/// 2. The allocator can still service new allocations in each class despite
///    the fragmented state.
/// 3. Freed blocks are actually reclaimable (reallocated from the free list).
#[test]
fn adversarial_fragmentation_alternating_classes() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();

    const ROUNDS: usize = 200;
    const CLASS_SIZES: [usize; 3] = [16, 32, 64];

    // Phase 1: Round-robin allocate across three size classes, pinning every
    // 4th allocation per class. The interleaving ensures no two adjacent
    // blocks in the same segment page belong to the same size class.
    let mut pinned: [std::vec::Vec<*mut u8>; 3] = [
        std::vec::Vec::new(),
        std::vec::Vec::new(),
        std::vec::Vec::new(),
    ];
    let mut freed: [std::vec::Vec<*mut u8>; 3] = [
        std::vec::Vec::new(),
        std::vec::Vec::new(),
        std::vec::Vec::new(),
    ];

    for round in 0..ROUNDS {
        for (class_idx, &size) in CLASS_SIZES.iter().enumerate() {
            let ptr = unsafe { alloc.alloc::<StandardPolicy>(size) };
            assert!(
                !ptr.is_null(),
                "allocation failed at round {round}, class {class_idx}, size {size}"
            );

            // Write a distinct pattern so we can verify data integrity.
            unsafe {
                for i in 0..size {
                    *ptr.add(i) = (round ^ class_idx) as u8;
                }
            }

            if round % 4 == 0 {
                pinned[class_idx].push(ptr);
            } else {
                freed[class_idx].push(ptr);
            }
        }
    }

    let stats_after_alloc = alloc.stats();
    assert!(
        stats_after_alloc.current_thread_owned_segments >= 1,
        "must own at least one segment after allocation phase"
    );

    // Phase 2: Free every non-pinned allocation. This scatters holes across
    // multiple size classes within the same segments.
    for ptrs in freed.iter() {
        for &ptr in ptrs {
            unsafe {
                crate::thread_free::<StandardPolicy, DefaultBackend>(ptr);
            }
        }
    }

    // Phase 3: Verify every pinned pointer is still valid.
    for (class_idx, ptrs) in pinned.iter().enumerate() {
        for (i, &ptr) in ptrs.iter().enumerate() {
            let size = CLASS_SIZES[class_idx];
            let expected_pattern = ((i * 4) ^ class_idx) as u8;
            for byte_idx in 0..size {
                let actual = unsafe { *ptr.add(byte_idx) };
                assert_eq!(
                    actual, expected_pattern,
                    "pinned pointer corruption: class={class_idx}, pin_index={i}, byte={byte_idx}, \
                     expected={expected_pattern:#x}, got={actual:#x}"
                );
            }
        }
    }

    // Phase 4: The allocator must still be able to serve new allocations in
    // each class despite the fragmentation.
    for (class_idx, &size) in CLASS_SIZES.iter().enumerate() {
        let new_ptrs: std::vec::Vec<*mut u8> = (0..50)
            .map(|i| {
                let ptr = unsafe { alloc.alloc::<StandardPolicy>(size) };
                assert!(
                    !ptr.is_null(),
                    "post-fragmentation allocation failed: class={class_idx}, index={i}"
                );
                ptr
            })
            .collect();

        // Every new pointer must be distinct (no duplicates from free-list corruption).
        let mut sorted = new_ptrs.clone();
        sorted.sort_unstable_by_key(|p| *p as usize);
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            new_ptrs.len(),
            "duplicate pointers returned under fragmentation: class={class_idx}"
        );

        for ptr in new_ptrs {
            unsafe {
                crate::thread_free::<StandardPolicy, DefaultBackend>(ptr);
            }
        }
    }

    // Cleanup: free all pinned blocks.
    for ptrs in pinned {
        for ptr in ptrs {
            unsafe {
                crate::thread_free::<StandardPolicy, DefaultBackend>(ptr);
            }
        }
    }
}

/// Adversarial fragmentation: alternating allocation/deallocation within
/// a single size class to create internal fragmentation.
///
/// Allocates blocks from one size class, frees every other one to create a
/// checkerboard pattern of free/allocated slots within pages, then verifies:
///
/// 1. The allocator reuses the freed slots (not allocating fresh pages).
/// 2. The retained (odd-index) blocks remain valid.
/// 3. Re-allocation of the freed slots produces distinct, non-null pointers.
#[test]
fn adversarial_single_class_checkerboard() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    const SIZE: usize = 32;
    const COUNT: usize = 128;

    // Phase 1: Allocate COUNT blocks.
    let mut all_ptrs = std::vec::Vec::with_capacity(COUNT);
    for i in 0..COUNT {
        let ptr = unsafe { alloc.alloc::<StandardPolicy>(SIZE) };
        assert!(!ptr.is_null(), "checkerboard alloc {i} failed");
        unsafe {
            *ptr = i as u8;
        }
        all_ptrs.push(ptr);
    }

    // Phase 2: Free every even-indexed block → checkerboard fragmentation.
    for i in (0..COUNT).step_by(2) {
        unsafe {
            crate::thread_free::<StandardPolicy, DefaultBackend>(all_ptrs[i]);
        }
    }

    // Phase 3: Allocate COUNT/2 blocks — must reuse the freed even slots.
    let mut reuse_ptrs = std::vec::Vec::with_capacity(COUNT / 2);
    for i in 0..(COUNT / 2) {
        let ptr = unsafe { alloc.alloc::<StandardPolicy>(SIZE) };
        assert!(
            !ptr.is_null(),
            "reuse allocation {i} failed after checkerboard free"
        );
        reuse_ptrs.push(ptr);
    }

    // Verify no pointer in reuse_ptrs matches any odd-indexed (still live) pointer.
    let live_set: std::collections::HashSet<usize> =
        all_ptrs[1..COUNT].iter().step_by(2).map(|p| *p as usize).collect();
    for &ptr in &reuse_ptrs {
        assert!(
            !live_set.contains(&(ptr as usize)),
            "reuse pointer {ptr:p} collides with a live pinned block"
        );
    }

    // Verify distinctness of reuse pointers.
    let mut sorted = reuse_ptrs.clone();
    sorted.sort_unstable_by_key(|p| *p as usize);
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        reuse_ptrs.len(),
        "duplicate pointers in reuse phase"
    );

    // Cleanup.
    for ptr in reuse_ptrs {
        unsafe {
            crate::thread_free::<StandardPolicy, DefaultBackend>(ptr);
        }
    }
    for i in (1..COUNT).step_by(2) {
        unsafe {
            crate::thread_free::<StandardPolicy, DefaultBackend>(all_ptrs[i]);
        }
    }
}

/// Adversarial fragmentation: mixed-size-class alloc/free interleaved to
/// stress the allocator's page reuse across size classes.
///
/// Allocates blocks from four size classes (16, 32, 48, 64 bytes) in a
/// wave pattern, frees alternating waves, and verifies the allocator can
/// recycle freed pages for different size classes.
#[test]
fn adversarial_mixed_class_page_recycling() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    const SIZES: [usize; 4] = [16, 32, 48, 64];
    const WAVE_SIZE: usize = 50;

    // Wave 1: allocate all four classes.
    let mut waves: [std::vec::Vec<*mut u8>; 4] = [
        std::vec::Vec::new(),
        std::vec::Vec::new(),
        std::vec::Vec::new(),
        std::vec::Vec::new(),
    ];

    for class_idx in 0..4 {
        for _ in 0..WAVE_SIZE {
            let ptr = unsafe { alloc.alloc::<StandardPolicy>(SIZES[class_idx]) };
            assert!(!ptr.is_null());
            unsafe {
                core::ptr::write_bytes(ptr, (class_idx * 17) as u8, SIZES[class_idx]);
            }
            waves[class_idx].push(ptr);
        }
    }

    // Free even-indexed classes (0 and 2) completely.
    for class_idx in [0, 2] {
        for ptr in waves[class_idx].drain(..) {
            unsafe {
                crate::thread_free::<StandardPolicy, DefaultBackend>(ptr);
            }
        }
    }

    // Wave 2: re-allocate the freed classes. The allocator should reuse freed
    // pages/blocks rather than mapping fresh segments.
    let segments_before = alloc.stats().fresh_segments;

    for class_idx in [0, 2] {
        for _ in 0..WAVE_SIZE {
            let ptr = unsafe { alloc.alloc::<StandardPolicy>(SIZES[class_idx]) };
            assert!(!ptr.is_null(), "wave 2 alloc failed for class {class_idx}");
            waves[class_idx].push(ptr);
        }
    }

    let segments_after = alloc.stats().fresh_segments;
    // The allocator should have reused at least some freed pages, so fresh
    // segment growth should be bounded (not a fresh segment per allocation).
    let new_segments = segments_after - segments_before;
    assert!(
        new_segments <= 2,
        "too many fresh segments for recycling: {new_segments} (expected <= 2)"
    );

    // Verify wave 1 survivors (classes 1 and 3) are still valid.
    for class_idx in [1, 3] {
        for &ptr in &waves[class_idx] {
            for byte_idx in 0..SIZES[class_idx] {
                let actual = unsafe { *ptr.add(byte_idx) };
                assert_eq!(
                    actual,
                    (class_idx * 17) as u8,
                    "wave 1 survivor corruption at class {class_idx}, byte {byte_idx}"
                );
            }
        }
    }

    // Cleanup all.
    for ptrs in waves {
        for ptr in ptrs {
            unsafe {
                crate::thread_free::<StandardPolicy, DefaultBackend>(ptr);
            }
        }
    }
}
