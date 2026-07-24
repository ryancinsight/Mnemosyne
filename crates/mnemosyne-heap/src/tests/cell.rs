use super::*;

#[test]
fn test_branded_cell_sharing_and_mutation() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let block = heap.alloc_init(&token, 42).expect("alloc_init failed");
        let cell = unsafe { BrandedCell::from_block(block) };

        // Cell is Copy/Clone, create multiple copies
        let cell_copy1 = cell;
        let cell_copy2 = cell;

        // Read original value
        assert_eq!(*cell_copy1.borrow(&token), 42);
        assert_eq!(*cell_copy2.borrow(&token), 42);

        // Mutate value via mutable borrow
        *cell_copy1.borrow_mut(&mut token) = 100;

        // Verify mutation is reflected in all copies
        assert_eq!(*cell_copy2.borrow(&token), 100);
        assert_eq!(*cell.borrow(&token), 100);

        // Free the memory using the safe/encapsulated conversion
        heap.free(&mut token, unsafe { cell.into_block() });
    });
}

#[test]
fn test_branded_cell_unsized_slice() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::new(&heap);
        vec.push(&mut token, 10)
            .expect("first vector push before unsized cell conversion failed");
        vec.push(&mut token, 20)
            .expect("second vector push before unsized cell conversion failed");
        vec.push(&mut token, 30)
            .expect("third vector push before unsized cell conversion failed");

        let boxed_slice = vec.into_boxed_slice(&mut token);
        assert_eq!(boxed_slice.len(), 3);

        let cell = boxed_slice.into_cell();
        assert_eq!(cell.borrow(&token), &[10, 20, 30]);

        // Mutate cell slice elements
        cell.borrow_mut(&mut token)[1] = 99;
        assert_eq!(cell.borrow(&token), &[10, 99, 30]);

        heap.free(&mut token, unsafe { cell.into_block() });
    });
}

#[test]
fn test_branded_heap_free_drops_unsized_value_before_reclaim() {
    let counter = std::sync::atomic::AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::new(&heap);
        for _ in 0..3 {
            vec.push(&mut token, DropTracker(&counter))
                .expect("unsized free regression allocation failed");
        }

        let cell = vec.into_cell(&mut token);
        heap.free(&mut token, unsafe { cell.into_block() });
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    });
}

#[test]
fn test_branded_cell_multi_mutable_borrow() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let b1 = heap
            .alloc_init(&token, 1)
            .expect("first branded cell allocation failed");
        let b2 = heap
            .alloc_init(&token, 2.0)
            .expect("second branded cell allocation failed");
        let b3 = heap
            .alloc_init(&token, std::string::String::from("3"))
            .expect("third branded cell allocation failed");

        let c1 = unsafe { BrandedCell::from_block(b1) };
        let c2 = unsafe { BrandedCell::from_block(b2) };
        let c3 = unsafe { BrandedCell::from_block(b3) };

        {
            let (r1, r2, r3) = BrandedCell::borrow_mut_3(&c1, &c2, &c3, &mut token);
            *r1 = 10;
            *r2 = 20.0;
            r3.push('0');
        }

        assert_eq!(*c1.borrow(&token), 10);
        assert_eq!(*c2.borrow(&token), 20.0);
        assert_eq!(c3.borrow(&token), "30");

        let (r1, r2) = BrandedCell::borrow_mut_2(&c1, &c2, &mut token);
        *r1 = 100;
        *r2 = 200.0;

        assert_eq!(*c1.borrow(&token), 100);
        assert_eq!(*c2.borrow(&token), 200.0);

        // Reclaim
        heap.free(&mut token, unsafe { c1.into_block() });
        heap.free(&mut token, unsafe { c2.into_block() });
        heap.free(&mut token, unsafe { c3.into_block() });
    });
}

#[test]
#[should_panic(expected = "borrow_mut_2: cells must be distinct")]
fn test_branded_cell_multi_mutable_borrow_panic() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let b = heap
            .alloc_init(&token, 42)
            .expect("branded cell panic-test allocation failed");
        let c = unsafe { BrandedCell::from_block(b) };

        // This must panic since c and c point to the same block
        let _ = BrandedCell::borrow_mut_2(&c, &c, &mut token);
    });
}

#[test]
fn test_branded_cell_as_ptr_identity() {
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let b1 = heap
            .alloc_init(&token, 42)
            .expect("first branded cell pointer-identity allocation failed");
        let b2 = heap
            .alloc_init(&token, 42)
            .expect("second branded cell pointer-identity allocation failed");

        let c1 = unsafe { BrandedCell::from_block(b1) };
        let c2 = unsafe { BrandedCell::from_block(b2) };
        let c1_copy = c1;

        assert_eq!(c1.as_ptr(), c1_copy.as_ptr());
        assert_ne!(c1.as_ptr(), c2.as_ptr());

        heap.free(&mut token, unsafe { c1.into_block() });
        heap.free(&mut token, unsafe { c2.into_block() });
    });
}

#[test]
fn test_branded_vec_into_cell() {
    let counter = std::sync::atomic::AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::new(&heap);
        vec.push(&mut token, DropTracker(&counter))
            .expect("first vector push before cell conversion failed");
        vec.push(&mut token, DropTracker(&counter))
            .expect("second vector push before cell conversion failed");

        let cell = vec.into_cell(&mut token);
        assert_eq!(cell.borrow(&token).len(), 2);
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        heap.free(&mut token, unsafe { cell.into_block() });
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    });
}
