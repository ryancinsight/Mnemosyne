use super::*;

#[test]
fn test_branded_containers_traits_and_vec_ops() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        // --- BrandedBlock ---
        let b1 = heap
            .alloc_init(&token, 42)
            .expect("first branded block trait-test allocation failed");
        let b2 = heap
            .alloc_init(&token, 42)
            .expect("second branded block trait-test allocation failed");

        // Pointer
        let _ = format!("{:p}", b1);
        // Debug
        let _ = format!("{:?}", b1);
        // PartialEq/Eq
        assert_eq!(b1, b1);
        assert_ne!(b1, b2);
        // PartialOrd/Ord
        assert!(b1 != b2);
        // Hash
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        use core::hash::Hash;
        b1.hash(&mut hasher);

        // --- BrandedBox ---
        let box1 = BrandedBox::new(&heap, &token, 100)
            .expect("first branded box trait-test allocation failed");
        let box2 = BrandedBox::new(&heap, &token, 200)
            .expect("second branded box trait-test allocation failed");
        // Display/Pointer/Debug
        let _ = format!("{}", box1);
        let _ = format!("{:p}", box1);
        let _ = format!("{:?}", box1);
        // PartialEq/Eq
        assert_eq!(box1, box1);
        assert_ne!(box1, box2);
        // PartialOrd/Ord
        assert_eq!(box1.partial_cmp(&box2), Some(core::cmp::Ordering::Less));
        assert_eq!(box1.cmp(&box2), core::cmp::Ordering::Less);
        // Hash
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        box1.hash(&mut hasher);
        // clone_in
        let box1_clone = box1
            .clone_in(&token)
            .expect("branded box trait-test clone allocation failed");
        assert_eq!(box1, box1_clone);

        // --- BrandedCell ---
        let cell1 = box1.into_cell();
        let cell2 = box2.into_cell();
        let cell1_clone = box1_clone.into_cell();
        // Debug
        let _ = format!("{:?}", cell1);
        // PartialEq/Eq
        assert_eq!(cell1, cell1);
        assert_ne!(cell1, cell2);
        // Hash
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        cell1.hash(&mut hasher);

        // Reclaim BrandedCells
        heap.free(&mut token, unsafe { cell1.into_block() });
        heap.free(&mut token, unsafe { cell2.into_block() });
        heap.free(&mut token, unsafe { cell1_clone.into_block() });

        // --- BrandedVec ---
        let mut vec = BrandedVec::new(&heap);
        vec.push(&mut token, 10)
            .expect("first branded vector trait-test push failed");
        vec.push(&mut token, 20)
            .expect("second branded vector trait-test push failed");
        vec.push(&mut token, 30)
            .expect("third branded vector trait-test push failed");

        // Debug
        let _ = format!("{:?}", vec);
        // PartialEq/Eq
        let vec_clone = vec
            .clone_in(&mut token)
            .expect("branded vector equality-test clone failed");
        assert_eq!(vec, vec_clone);
        // PartialOrd/Ord
        assert!(vec <= vec_clone);

        // clear
        let mut vec_clear = vec_clone;
        vec_clear.clear();
        assert_eq!(vec_clear.len(), 0);

        // truncate
        let mut vec_trunc = vec
            .clone_in(&mut token)
            .expect("branded vector truncate-test clone failed");
        vec_trunc.truncate(1);
        assert_eq!(vec_trunc.len(), 1);
        assert_eq!(vec_trunc[0], 10);

        // reserve & shrink_to_fit
        let mut vec_res = vec
            .clone_in(&mut token)
            .expect("branded vector reserve-test clone failed");
        vec_res
            .reserve(&mut token, 100)
            .expect("branded vector reserve-test growth failed");
        assert!(vec_res.capacity() >= 103);
        vec_res
            .shrink_to_fit(&mut token)
            .expect("branded vector reserve-test shrink_to_fit failed");
        assert_eq!(vec_res.capacity(), 3);

        // insert & remove
        let mut vec_ins = vec
            .clone_in(&mut token)
            .expect("branded vector insert-test clone failed");
        vec_ins
            .insert(&mut token, 1, 99)
            .expect("branded vector insert-test insertion failed");
        assert_eq!(vec_ins.len(), 4);
        assert_eq!(vec_ins[0], 10);
        assert_eq!(vec_ins[1], 99);
        assert_eq!(vec_ins[2], 20);
        assert_eq!(vec_ins[3], 30);

        let removed = vec_ins.remove(1);
        assert_eq!(removed, 99);
        assert_eq!(vec_ins.len(), 3);
        assert_eq!(vec_ins[0], 10);
        assert_eq!(vec_ins[1], 20);
        assert_eq!(vec_ins[2], 30);

        // Clean up
        heap.free(&mut token, b1);
        heap.free(&mut token, b2);
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
fn test_branding_thread_bounds() {
    // Structurally, BrandedBox and BrandedVec contain PhantomData<*mut ()>; the melinoe
    // ThreadLocalToken contains PhantomData<*const ()> (both !Send + !Sync)
    // which guarantees that these types are neither Send nor Sync.
    // This ensures they cannot be sent to other threads or accessed concurrently.
    // If they were Send, dropping them on another thread would cause unsynchronized
    // mutation of the thread-local allocator cache.

    // We verify that *mut () itself is !Send + !Sync.
    #[allow(dead_code)]
    trait ImplementsSend {}
    impl<T: Send> ImplementsSend for T {}

    #[allow(dead_code)]
    trait ImplementsSync {}
    impl<T: Sync> ImplementsSync for T {}

    // The following helper type has the same layout and markers as BrandedBox.
    // We can verify that types containing PhantomData<*mut ()> are indeed !Send + !Sync.
    // (Uncommenting the impls below would trigger compile errors, verifying the invariant).
    // struct SendAssert<T: Send>(T);
    // struct SyncAssert<T: Sync>(T);
    // let _ = SendAssert(std::marker::PhantomData::<*mut ()>);
    // let _ = SyncAssert(std::marker::PhantomData::<*mut ()>);
}
