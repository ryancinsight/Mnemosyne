use super::*;

#[test]
fn test_branded_vec_growth_and_drop() {
    let counter = AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::new(&heap);
        assert!(vec.is_empty());
        assert_eq!(vec.len(), 0);
        assert_eq!(vec.capacity(), 0);

        // Push elements to trigger growth
        for _ in 0..10 {
            vec.push(&mut token, DropTracker(&counter))
                .expect("branded vector growth push failed");
        }
        assert_eq!(vec.len(), 10);
        assert!(vec.capacity() >= 10);

        // Pop half of the elements
        for _ in 0..5 {
            let popped = vec.pop();
            assert!(popped.is_some());
            drop(popped);
        }
        assert_eq!(vec.len(), 5);
        assert_eq!(counter.load(Ordering::SeqCst), 5); // 5 popped elements dropped

        // Drop vec, remainder should drop
        drop(vec);
        assert_eq!(counter.load(Ordering::SeqCst), 10); // all 10 elements dropped
    });
}

#[test]
fn test_branded_vec_zst_uses_sentinel_capacity_and_drops_elements() {
    ZST_DROP_COUNT.with(|c| c.set(0));
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let before = heap.stats().current_thread_owned_segments;
        let mut vec =
            BrandedVec::with_capacity(&heap, &token, 8).expect("ZST vector construction failed");
        assert_eq!(vec.capacity(), usize::MAX);
        assert_eq!(
            heap.stats().current_thread_owned_segments,
            before,
            "ZST vector capacity must not allocate a segment"
        );

        for _ in 0..4 {
            vec.push(&mut token, ZstDrop).expect("ZST push failed");
        }
        assert_eq!(vec.len(), 4);
        drop(vec.pop());
        assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 1);
        drop(vec);
        assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 4);
    });
}

#[test]
fn test_branded_vec_new_zst_preserves_capacity_invariant() {
    ZST_DROP_COUNT.with(|c| c.set(0));
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let before = heap.stats().current_thread_owned_segments;
        let mut vec = BrandedVec::new(&heap);

        assert_eq!(
            vec.capacity(),
            usize::MAX,
            "ZST vector constructed with new must use sentinel capacity"
        );
        vec.push(&mut token, ZstDrop).expect("ZST push failed");
        assert_eq!(vec.len(), 1);
        assert!(
            vec.len() <= vec.capacity(),
            "successful push must preserve len <= capacity"
        );
        assert_eq!(
            heap.stats().current_thread_owned_segments,
            before,
            "ZST vector constructed with new must not allocate a segment"
        );

        drop(vec);
        assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 1);
    });
}

#[test]
fn test_branded_vec_into_boxed_slice_shrinks_storage_to_len() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::with_capacity(&heap, &token, 1024)
            .expect("oversized vector allocation failed");
        vec.push(&mut token, 0xCAFE_BABEu64)
            .expect("push into preallocated vector failed");

        let before_usable = unsafe { mnemosyne_local::usable_size(vec.as_ptr() as *mut u8) };
        let boxed_slice = vec.into_boxed_slice(&mut token);
        let after_usable = unsafe { mnemosyne_local::usable_size(boxed_slice.as_ptr() as *mut u8) };

        assert_eq!(boxed_slice.len(), 1);
        assert_eq!(boxed_slice[0], 0xCAFE_BABE);
        assert!(
            after_usable <= before_usable,
            "boxed slice conversion must not increase usable storage from {before_usable}, got {after_usable}"
        );
    });
}

#[test]
fn test_branded_vec_from_boxed_slice_transitions() {
    let counter = std::sync::atomic::AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        // Sized types test
        let mut vec = BrandedVec::new(&heap);
        vec.push(&mut token, DropTracker(&counter))
            .expect("first sized vector push before boxed-slice transition failed");
        vec.push(&mut token, DropTracker(&counter))
            .expect("second sized vector push before boxed-slice transition failed");

        let boxed = vec.into_boxed_slice(&mut token);
        assert_eq!(boxed.len(), 2);
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        let vec_recovered = BrandedVec::from_boxed_slice(boxed);
        assert_eq!(vec_recovered.len(), 2);
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        drop(vec_recovered);
        assert_eq!(counter.load(Ordering::SeqCst), 2);

        // ZST test
        let mut zst_vec = BrandedVec::new(&heap);
        zst_vec
            .push(&mut token, ())
            .expect("first ZST vector push before boxed-slice transition failed");
        zst_vec
            .push(&mut token, ())
            .expect("second ZST vector push before boxed-slice transition failed");

        let zst_boxed = zst_vec.into_boxed_slice(&mut token);
        assert_eq!(zst_boxed.len(), 2);

        let zst_vec_recovered = BrandedVec::from_boxed_slice(zst_boxed);
        assert_eq!(zst_vec_recovered.len(), 2);
        assert_eq!(zst_vec_recovered.capacity(), usize::MAX);
    });
}

#[test]
fn test_branded_vec_in_place_shrink() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        // Minor shrink (within 50% threshold): should shrink in-place
        let mut vec_minor = BrandedVec::with_capacity(&heap, &token, 4)
            .expect("minor-shrink vector allocation failed");
        vec_minor
            .push(&mut token, 42)
            .expect("minor-shrink first push failed");
        vec_minor
            .push(&mut token, 43)
            .expect("minor-shrink second push failed");
        vec_minor
            .push(&mut token, 44)
            .expect("minor-shrink third push failed");

        let orig_ptr_minor = vec_minor.as_slice().as_ptr();
        assert_eq!(vec_minor.capacity(), 4);

        // Shrink minor vector capacity from 4 to 3 (new_size 12 >= 16 / 2)
        vec_minor
            .shrink_to_fit(&mut token)
            .expect("minor in-place shrink_to_fit failed");
        assert_eq!(vec_minor.len(), 3);
        assert_eq!(vec_minor.capacity(), 3);
        assert_eq!(vec_minor.as_slice().as_ptr(), orig_ptr_minor);

        // Major shrink (below 50% threshold): should copy & free to release memory
        let mut vec_major = BrandedVec::with_capacity(&heap, &token, 10)
            .expect("major-shrink vector allocation failed");
        vec_major
            .push(&mut token, 100)
            .expect("major-shrink first push failed");
        vec_major
            .push(&mut token, 101)
            .expect("major-shrink second push failed");

        let orig_ptr_major = vec_major.as_slice().as_ptr();
        assert_eq!(vec_major.capacity(), 10);

        // Shrink major vector capacity from 10 to 2 (new_size 8 < 40 / 2)
        vec_major
            .shrink_to_fit(&mut token)
            .expect("major copy-shrink shrink_to_fit failed");
        assert_eq!(vec_major.len(), 2);
        assert_eq!(vec_major.capacity(), 2);
        assert_ne!(vec_major.as_slice().as_ptr(), orig_ptr_major);

        // Similar minor shrink check for into_boxed_slice
        let mut vec_slice = BrandedVec::with_capacity(&heap, &token, 4)
            .expect("boxed-slice shrink vector allocation failed");
        vec_slice
            .push(&mut token, 200)
            .expect("boxed-slice shrink first push failed");
        vec_slice
            .push(&mut token, 201)
            .expect("boxed-slice shrink second push failed");
        vec_slice
            .push(&mut token, 202)
            .expect("boxed-slice shrink third push failed");

        let orig_ptr_slice = vec_slice.as_slice().as_ptr();
        let boxed_slice = vec_slice.into_boxed_slice(&mut token);
        assert_eq!(boxed_slice.len(), 3);
        assert_eq!((*boxed_slice).as_ptr(), orig_ptr_slice);
    });
}

#[test]
fn test_branded_vec_extensions() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::new(&heap);
        assert_eq!(vec.len(), 0);

        // Test extend
        vec.extend(&mut token, std::vec![10, 20, 30])
            .expect("branded vector extend failed");
        assert_eq!(vec.len(), 3);
        assert_eq!(vec[0], 10);
        assert_eq!(vec[1], 20);
        assert_eq!(vec[2], 30);

        // Test extend_from_slice
        vec.extend_from_slice(&mut token, &[40, 50])
            .expect("branded vector extend_from_slice failed");
        assert_eq!(vec.len(), 5);
        assert_eq!(vec[3], 40);
        assert_eq!(vec[4], 50);

        // Test resize (grow)
        vec.resize(&mut token, 7, 99)
            .expect("branded vector grow resize failed");
        assert_eq!(vec.len(), 7);
        assert_eq!(vec[5], 99);
        assert_eq!(vec[6], 99);

        // Test resize (shrink)
        vec.resize(&mut token, 4, 99)
            .expect("branded vector shrink resize failed");
        assert_eq!(vec.len(), 4);
        assert_eq!(vec[0], 10);
        assert_eq!(vec[1], 20);
        assert_eq!(vec[2], 30);
        assert_eq!(vec[3], 40);
    });
}
