use super::*;

#[test]
fn realloc_within_usable_size_returns_same_pointer_and_preserves_bytes() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let old_layout =
        Layout::from_size_align(24, 8).expect("24-byte 8-byte aligned Layout is valid");
    let ptr = unsafe { ALLOCATOR.alloc(old_layout) };
    assert!(!ptr.is_null(), "realloc setup allocation failed");
    unsafe {
        core::ptr::write_bytes(ptr, 0xA5, old_layout.size());
    }

    let usable = unsafe { usable_size(ptr) };
    assert!(
        usable >= 32,
        "test requires allocation usable size >= 32, got {usable}"
    );
    let new_ptr = unsafe { ALLOCATOR.realloc(ptr, old_layout, 32) };
    assert_eq!(
        new_ptr, ptr,
        "standard realloc within usable size should stay in place"
    );
    for offset in 0..old_layout.size() {
        let byte = unsafe { *new_ptr.add(offset) };
        assert_eq!(byte, 0xA5, "realloc failed to preserve byte {offset}");
    }

    let new_layout =
        Layout::from_size_align(32, 8).expect("32-byte 8-byte aligned Layout is valid");
    unsafe { ALLOCATOR.dealloc(new_ptr, new_layout) };
}

#[test]
fn secure_realloc_within_usable_size_uses_replacement_allocation() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let allocator = MnemosyneAllocator::<SecurePolicy>::new();
    let old_layout =
        Layout::from_size_align(24, 8).expect("24-byte 8-byte aligned Layout is valid");
    let ptr = unsafe { allocator.alloc(old_layout) };
    assert!(!ptr.is_null(), "secure realloc setup allocation failed");
    unsafe {
        core::ptr::write_bytes(ptr, 0x5A, old_layout.size());
    }

    let new_ptr = unsafe { allocator.realloc(ptr, old_layout, 32) };
    assert!(
        !new_ptr.is_null(),
        "secure realloc returned null for in-class growth"
    );
    assert_eq!(
        new_ptr, ptr,
        "secure realloc must grow in place within the same size class block"
    );
    for offset in 0..old_layout.size() {
        let byte = unsafe { *new_ptr.add(offset) };
        assert_eq!(
            byte, 0x5A,
            "secure realloc failed to preserve byte {offset}"
        );
    }
    for offset in old_layout.size()..32 {
        let byte = unsafe { *new_ptr.add(offset) };
        assert_eq!(byte, 0, "secure realloc failed to zero new byte {offset}");
    }

    let new_layout =
        Layout::from_size_align(32, 8).expect("32-byte 8-byte aligned Layout is valid");
    unsafe { allocator.dealloc(new_ptr, new_layout) };
}

#[test]
fn realloc_zero_size_returns_null_without_allocating() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let layout = Layout::from_size_align(24, 8).expect("24-byte 8-byte aligned Layout is valid");
    let ptr = unsafe { ALLOCATOR.alloc(layout) };
    assert!(!ptr.is_null(), "zero-size realloc setup allocation failed");
    let new_ptr = unsafe { ALLOCATOR.realloc(ptr, layout, 0) };
    assert!(
        new_ptr.is_null(),
        "zero-size realloc returned non-null pointer {new_ptr:?}"
    );

    let null_realloc = unsafe { ALLOCATOR.realloc(core::ptr::null_mut(), layout, 0) };
    assert!(
        null_realloc.is_null(),
        "null zero-size realloc returned non-null pointer {null_realloc:?}"
    );
}

#[test]
fn test_realloc_within_class_returns_same_ptr() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    // 32 B request lands in size class 1 (block_size = 32 B); shrinking
    // and growing-within-class must both return the same pointer with
    // no copy-and-free.
    let layout = Layout::from_size_align(32, 8).expect("valid layout");
    let ptr = unsafe { ALLOCATOR.alloc(layout) };
    assert!(!ptr.is_null());

    // Mark a sentinel byte so we can detect any unintended copy.
    unsafe { ptr.write(0x5A) };

    // Shrink within class.
    let shrunk = unsafe { ALLOCATOR.realloc(ptr, layout, 16) };
    assert_eq!(
        shrunk, ptr,
        "shrink within class must return the same pointer"
    );

    // Grow within class.
    let grown = unsafe { ALLOCATOR.realloc(shrunk, layout, 32) };
    assert_eq!(grown, ptr, "grow within class must return the same pointer");

    // Confirm the sentinel survived — no copy happened.
    assert_eq!(
        unsafe { ptr.read() },
        0x5A,
        "sentinel byte mutated; an unwanted copy occurred"
    );

    unsafe { ALLOCATOR.dealloc(ptr, layout) };
}

#[test]
fn test_realloc_large_half_shrink_returns_same_ptr() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let old_layout = Layout::from_size_align(4 * 1024 * 1024, 8).expect("valid layout");
    let new_size = 2 * 1024 * 1024;
    let ptr = unsafe { ALLOCATOR.alloc(old_layout) };
    assert!(!ptr.is_null(), "large realloc setup allocation failed");

    unsafe {
        ptr.write(0xC3);
        ptr.add(new_size - 1).write(0x3C);
    }

    let shrunk = unsafe { ALLOCATOR.realloc(ptr, old_layout, new_size) };
    assert_eq!(
        shrunk, ptr,
        "standard half-shrink must avoid allocate-copy-free churn"
    );
    assert_eq!(unsafe { shrunk.read() }, 0xC3);
    assert_eq!(unsafe { shrunk.add(new_size - 1).read() }, 0x3C);

    let new_layout = Layout::from_size_align(new_size, 8).expect("valid layout");
    unsafe { ALLOCATOR.dealloc(shrunk, new_layout) };
}

#[test]
fn test_realloc_across_class_copies_and_returns_new_ptr() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    // 16 B request → class 0 (block_size 16). Growing to 64 B requires
    // a different size class; the realloc must allocate, copy, and
    // free. The original sentinel bytes must appear in the new
    // allocation.
    let small_layout = Layout::from_size_align(16, 8).expect("valid layout");
    let ptr = unsafe { ALLOCATOR.alloc(small_layout) };
    assert!(!ptr.is_null());

    // Fill the 16 B with a known pattern.
    for i in 0..16usize {
        unsafe { ptr.add(i).write((i as u8).wrapping_add(0xA0)) };
    }

    let new_ptr = unsafe { ALLOCATOR.realloc(ptr, small_layout, 64) };
    assert!(!new_ptr.is_null());

    // The new allocation may or may not coincide with `ptr` depending
    // on the size-class choice; what matters is that the prefix
    // bytes were preserved.
    for i in 0..16usize {
        assert_eq!(
            unsafe { new_ptr.add(i).read() },
            (i as u8).wrapping_add(0xA0),
            "realloc across class did not preserve byte {i}"
        );
    }

    let new_layout = Layout::from_size_align(64, 8).expect("valid layout");
    unsafe { ALLOCATOR.dealloc(new_ptr, new_layout) };
}

#[test]
fn test_realloc_does_not_copy_past_layout_size() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    // Pins the slow-path copy-length contract: even when the caller's
    // allocation has size-class slack (usable_size > layout.size), the
    // slow path must copy *only* layout.size bytes. If it instead
    // copied usable_size bytes, an accidental write in the slack
    // region would propagate to the new allocation.
    //
    // Setup: 8 B request lands in class 0 (block_size 16 B), so
    // layout.size = 8 but usable_size(ptr) = 16. Use SecurePolicy so
    // the replacement allocation has defined zero bytes beyond the
    // copied user region; this lets the test inspect [8, 16) without
    // reading uninitialized memory.
    let allocator = MnemosyneAllocator::<SecurePolicy>::new();
    let small_layout = Layout::from_size_align(8, 8).expect("valid layout");
    let ptr = unsafe { allocator.alloc(small_layout) };
    assert!(!ptr.is_null());
    // Sanity-check the slack window exists.
    let reported = unsafe { usable_size(ptr) };
    assert!(
        reported >= 16,
        "8 B request must land in a class with at least 16 B usable; got {reported}"
    );

    // User region: bytes 0..8.
    for i in 0..8usize {
        unsafe { ptr.add(i).write(0xAA) };
    }
    // Slack region: bytes 8..16. Mnemosyne lets you safely write up to
    // usable_size bytes, so this is well-defined; but the realloc copy
    // must not pull this into the new allocation.
    for i in 8..16usize {
        unsafe { ptr.add(i).write(0xBB) };
    }

    // Cross-class grow.
    let new_ptr = unsafe { allocator.realloc(ptr, small_layout, 64) };
    assert!(!new_ptr.is_null());

    for i in 0..8usize {
        assert_eq!(
            unsafe { new_ptr.add(i).read() },
            0xAA,
            "realloc must preserve the {i}-th user byte"
        );
    }
    for i in 8..16usize {
        assert_eq!(
            unsafe { new_ptr.add(i).read() },
            0,
            "secure realloc copied slack byte {i} past layout.size"
        );
    }

    let new_layout = Layout::from_size_align(64, 8).expect("valid layout");
    unsafe { allocator.dealloc(new_ptr, new_layout) };
}

#[test]
fn test_realloc_shrink_replacement_copies_only_new_size() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let allocator = MnemosyneAllocator::<SecurePolicy>::new();
    let old_layout = Layout::from_size_align(16 * 1024, 8).expect("valid layout");
    let new_size = 4 * 1024;
    let ptr = unsafe { allocator.alloc(old_layout) };
    assert!(!ptr.is_null(), "secure shrink setup allocation failed");

    for i in 0..new_size {
        unsafe { ptr.add(i).write((i as u8).wrapping_mul(17)) };
    }

    let new_ptr = unsafe { allocator.realloc(ptr, old_layout, new_size) };
    assert!(
        !new_ptr.is_null(),
        "secure shrink replacement allocation failed"
    );
    for i in 0..new_size {
        assert_eq!(
            unsafe { new_ptr.add(i).read() },
            (i as u8).wrapping_mul(17),
            "secure shrink failed to preserve byte {i}"
        );
    }

    let new_layout = Layout::from_size_align(new_size, 8).expect("valid layout");
    unsafe { allocator.dealloc(new_ptr, new_layout) };
}

#[test]
fn test_realloc_null_ptr_acts_as_alloc() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let layout = Layout::from_size_align(0, 8).expect("valid layout");
    let ptr = unsafe { ALLOCATOR.realloc(core::ptr::null_mut(), layout, 128) };
    assert!(!ptr.is_null(), "realloc(null, 128) must allocate");
    let new_layout = Layout::from_size_align(128, 8).expect("valid layout");
    unsafe { ALLOCATOR.dealloc(ptr, new_layout) };
}

#[test]
fn test_realloc_to_zero_size_frees() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    let layout = Layout::from_size_align(32, 8).expect("valid layout");
    let ptr = unsafe { ALLOCATOR.alloc(layout) };
    assert!(!ptr.is_null());

    let null = unsafe { ALLOCATOR.realloc(ptr, layout, 0) };
    assert!(null.is_null(), "realloc(_, 0) must return null after free");
}

#[test]
fn test_realloc_preserves_alignment_for_aligned_small() {
    let _guard = TEST_LOCK
        .lock()
        .expect("global allocator test lock was poisoned");
    // A 64-byte-aligned small block grown to a size whose natural size class
    // stride is NOT a multiple of 64 (200 -> class 224). The realloc result must
    // still be 64-aligned and preserve the original bytes.
    unsafe {
        let layout = Layout::from_size_align(128, 64).expect("valid layout");
        let p = ALLOCATOR.alloc(layout);
        assert!(!p.is_null(), "initial aligned alloc failed");
        assert_eq!((p as usize) & 63, 0, "initial block not 64-aligned");
        core::ptr::write_bytes(p, 0xCD, 128);

        let p2 = ALLOCATOR.realloc(p, layout, 200);
        assert!(!p2.is_null(), "realloc failed");
        assert_eq!(
            (p2 as usize) & 63,
            0,
            "realloc must preserve 64-byte alignment"
        );
        assert_eq!(*p2, 0xCD, "realloc must preserve leading bytes");
        assert_eq!(*p2.add(127), 0xCD, "realloc must preserve trailing bytes");

        ALLOCATOR.dealloc(p2, Layout::from_size_align(200, 64).expect("valid layout"));
    }
}
