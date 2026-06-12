use super::*;

#[test]
fn test_branded_box_and_drop_tracking() {
    let counter = AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
        let bbox = BrandedBox::new(&heap, &token, DropTracker(&counter))
            .expect("BrandedBox allocation failed");
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        // Drop should occur here when bbox goes out of scope.
        drop(bbox);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn test_branded_box_zst_drops_without_allocating() {
    ZST_DROP_COUNT.with(|c| c.set(0));
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
        let before = heap.stats().current_thread_owned_segments;
        let bbox = BrandedBox::new(&heap, &token, ZstDrop).expect("ZST box allocation failed");
        let after_new = heap.stats().current_thread_owned_segments;
        assert_eq!(after_new, before, "ZST box must not allocate a segment");
        drop(bbox);
        assert_eq!(ZST_DROP_COUNT.with(|c| c.get()), 1);
    });
}

#[test]
fn test_branded_box_unsized_slice_and_drop() {
    let counter = AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let mut vec = BrandedVec::new(&heap);
        for _ in 0..5 {
            vec.push(&mut token, DropTracker(&counter))
                .expect("branded vector push before boxed-slice conversion failed");
        }
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Convert to boxed slice
        let boxed_slice = vec.into_boxed_slice(&mut token);
        assert_eq!(boxed_slice.len(), 5);
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Drop boxed slice
        drop(boxed_slice);
        assert_eq!(counter.load(Ordering::SeqCst), 5); // All 5 elements dropped
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
fn test_branded_box_into_and_from_raw() {
    let counter = AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
        let bbox = BrandedBox::new(&heap, &token, DropTracker(&counter))
            .expect("BrandedBox allocation failed");
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Convert to raw block
        let block = bbox.into_raw();
        assert_eq!(counter.load(Ordering::SeqCst), 0); // No drop yet

        // Reconstruct BrandedBox from raw block
        let bbox_reconstructed = unsafe { BrandedBox::from_raw(&heap, block) };
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Drop reconstructed box
        drop(bbox_reconstructed);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn test_branded_box_into_cell() {
    let counter = AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let bbox = BrandedBox::new(&heap, &token, DropTracker(&counter))
            .expect("BrandedBox allocation failed");
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Convert to shared BrandedCell
        let cell = bbox.into_cell();
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Read the cell
        assert_eq!(cell.borrow(&token).0.load(Ordering::SeqCst), 0);

        // Reclaim memory using the safe/encapsulated conversion
        heap.free(&mut token, unsafe { cell.into_block() });
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn test_branded_box_from_cell() {
    let counter = std::sync::atomic::AtomicUsize::new(0);
    scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, token| {
        let bbox =
            BrandedBox::new(&heap, &token, DropTracker(&counter)).expect("allocation failed");
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        let cell = bbox.into_cell();
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Reconstruct box from cell
        let bbox_reconstructed = unsafe { BrandedBox::from_cell(&heap, cell) };
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        drop(bbox_reconstructed);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    });
}
