use super::super::*;
use mnemosyne_core::constants::{PAGE_SHIFT, PAGES_PER_SEGMENT, SEGMENT_SIZE};
use mnemosyne_core::policy::StandardPolicy;

/// Helper: allocate a small block and return its pointer.
fn alloc_small(alloc: &mut ThreadAllocator<DefaultBackend>, size: usize) -> *mut u8 {
    let ptr = unsafe { alloc.alloc::<StandardPolicy>(size) };
    assert!(!ptr.is_null(), "alloc_small({size}) returned null");
    ptr
}

/// Helper: compute the size class block stride for a small allocation.
fn block_stride_for(ptr: *mut u8) -> usize {
    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut mnemosyne_core::types::Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    unsafe { (*segment).pages[page_index].block_size }
}

/// Realloc within the same size class returns the same pointer (in-place reuse).
///
/// Exercises the `small_realloc_fits_existing_class` fast path:
/// the new size rounds up to the same block stride, so the allocator
/// returns `ptr` unchanged without any copy.
#[test]
fn realloc_within_same_class_returns_same_ptr() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let ptr = alloc_small(&mut alloc, 16);
    let stride = block_stride_for(ptr);

    // Grow from 16 to a size still within the same class (stride).
    let new_size = stride - 1;
    let layout = core::alloc::Layout::from_size_align(16, 8).expect("layout valid");
    let result = unsafe { crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, new_size) };
    assert_eq!(
        result, ptr,
        "realloc within same class must return the same pointer (in-place)"
    );

    // Shrink to >= 50% of original size — also in-place.
    let half = 16 / 2;
    let result2 = unsafe { crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, half) };
    assert_eq!(
        result2, ptr,
        "realloc shrinking to >= 50% must return the same pointer"
    );

    unsafe { crate::thread_free::<StandardPolicy, DefaultBackend>(ptr) };
}

/// Realloc that shrinks below 50% allocates a new block, copies, and frees old.
///
/// The capacity-shrink heuristic forces a real allocation when new_size
/// is less than half the original size, even though the old block could
/// technically hold the data. This prevents indefinite retention of
/// oversized blocks.
///
/// When realloc returns a new pointer, it internally frees the old one.
#[test]
fn realloc_shrink_below_half_allocates_new_block() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let ptr = alloc_small(&mut alloc, 64);

    // Write a marker so we can verify the data is copied.
    unsafe {
        core::ptr::write_bytes(ptr, 0xAB, 64);
    }

    // Shrink to < 50% (32/64 = 50%, so use 16 which is < 50%).
    let layout = core::alloc::Layout::from_size_align(64, 8).expect("layout valid");
    let result = unsafe { crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, 16) };
    assert!(
        !result.is_null(),
        "shrink-below-half realloc must not return null"
    );
    assert_ne!(
        result, ptr,
        "shrink below 50% must allocate a new block"
    );

    // Verify the data was copied correctly.
    let slice = unsafe { core::slice::from_raw_parts(result, 16) };
    for (i, &byte) in slice.iter().enumerate() {
        assert_eq!(
            byte, 0xAB,
            "data mismatch at byte {i}: expected 0xAB, got {byte}"
        );
    }

    // realloc already freed ptr internally when returning a new block.
    unsafe { crate::thread_free::<StandardPolicy, DefaultBackend>(result) };
}

/// Realloc that grows beyond the current size class allocates a new block.
///
/// When the new size exceeds the current block's stride, the allocator
/// must allocate a new block in the appropriate class, copy min(old, new)
/// bytes, and free the old block.
#[test]
fn realloc_grow_to_different_class_allocates_new_block() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let ptr = alloc_small(&mut alloc, 16);

    // Write marker data.
    unsafe {
        core::ptr::write_bytes(ptr, 0xCD, 16);
    }

    let stride = block_stride_for(ptr);
    // Grow to a size larger than the current class stride.
    let new_size = stride + 1;
    let layout = core::alloc::Layout::from_size_align(16, 8).expect("layout valid");
    let result = unsafe { crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, new_size) };
    assert!(
        !result.is_null(),
        "grow-beyond-class realloc must not return null"
    );
    assert_ne!(
        result, ptr,
        "grow beyond class must allocate a new block"
    );

    // Verify the first 16 bytes (the original data) were copied.
    let slice = unsafe { core::slice::from_raw_parts(result, 16) };
    for (i, &byte) in slice.iter().enumerate() {
        assert_eq!(
            byte, 0xCD,
            "data mismatch at byte {i}: expected 0xCD, got {byte}"
        );
    }

    // realloc already freed ptr internally when returning a new block.
    unsafe { crate::thread_free::<StandardPolicy, DefaultBackend>(result) };
}

/// Realloc with null pointer behaves like alloc.
#[test]
fn realloc_null_ptr_allocates_fresh() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let layout = core::alloc::Layout::from_size_align(32, 8).expect("layout valid");
    let result = unsafe {
        crate::thread_realloc::<StandardPolicy, DefaultBackend>(
            core::ptr::null_mut(),
            layout,
            32,
        )
    };
    assert!(
        !result.is_null(),
        "realloc(null, 32) must return a valid pointer"
    );

    // Write and verify.
    unsafe {
        core::ptr::write_bytes(result, 0xEF, 32);
        assert_eq!(*result, 0xEF);
    }

    unsafe { crate::thread_free::<StandardPolicy, DefaultBackend>(result) };
}

/// Realloc with null pointer and size 0 returns null (no-op).
#[test]
fn realloc_null_ptr_zero_size_returns_null() {
    let layout = core::alloc::Layout::from_size_align(32, 8).expect("layout valid");
    let result = unsafe {
        crate::thread_realloc::<StandardPolicy, DefaultBackend>(
            core::ptr::null_mut(),
            layout,
            0,
        )
    };
    assert!(
        result.is_null(),
        "realloc(null, 0) must return null"
    );
}

/// Realloc with non-null pointer and size 0 frees the block.
#[test]
fn realloc_non_null_zero_size_frees_block() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let ptr = alloc_small(&mut alloc, 32);

    let layout = core::alloc::Layout::from_size_align(32, 8).expect("layout valid");
    let result = unsafe { crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, 0) };
    assert!(
        result.is_null(),
        "realloc(ptr, 0) must return null (free semantics)"
    );
}

/// Realloc grow within the same class preserves data for the original region.
#[test]
fn realloc_grow_within_class_preserves_existing_data() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let ptr = alloc_small(&mut alloc, 16);

    // Fill with a recognizable pattern.
    unsafe {
        for i in 0..16 {
            *ptr.add(i) = (i as u8) ^ 0x55;
        }
    }

    let stride = block_stride_for(ptr);
    // Grow to a size still within the same class.
    let new_size = stride - 1;
    let layout = core::alloc::Layout::from_size_align(16, 8).expect("layout valid");
    let result = unsafe { crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, new_size) };
    assert_eq!(
        result, ptr,
        "grow within same class must reuse the block"
    );

    // Verify the original 16 bytes are intact.
    for i in 0..16 {
        let expected = (i as u8) ^ 0x55;
        let actual = unsafe { *ptr.add(i) };
        assert_eq!(
            actual, expected,
            "byte {i} corrupted: expected {expected:#x}, got {actual:#x}"
        );
    }

    unsafe { crate::thread_free::<StandardPolicy, DefaultBackend>(ptr) };
}

/// Realloc grow beyond the class copies only min(old, new) bytes.
///
/// When growing from a small class to a larger one, the copy length is
/// `min(old_size, new_size)`. This test verifies that bytes beyond the
/// old size are NOT read during the copy (they would be uninitialized).
#[test]
fn realloc_grow_beyond_class_copies_only_min_bytes() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let ptr = alloc_small(&mut alloc, 16);

    // Write only 16 bytes.
    unsafe {
        core::ptr::write_bytes(ptr, 0x77, 16);
    }

    let stride = block_stride_for(ptr);
    let new_size = stride + 16;
    let layout = core::alloc::Layout::from_size_align(16, 8).expect("layout valid");
    let result = unsafe { crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, new_size) };
    assert!(!result.is_null());

    // The first 16 bytes must match.
    for i in 0..16 {
        let actual = unsafe { *result.add(i) };
        assert_eq!(
            actual, 0x77,
            "byte {i} mismatch: expected 0x77, got {actual:#x}"
        );
    }

    // realloc already freed ptr internally.
    unsafe { crate::thread_free::<StandardPolicy, DefaultBackend>(result) };
}

/// Realloc shrink followed by grow returns to the same pointer if within class.
///
/// This exercises the bidirectional in-place reuse: first a shrink (>= 50%),
/// then a grow back to the original size — both should reuse the same block.
#[test]
fn realloc_shrink_then_grow_stays_in_place() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let ptr = alloc_small(&mut alloc, 32);
    let layout = core::alloc::Layout::from_size_align(32, 8).expect("layout valid");

    // Shrink to 50% (>= 50% threshold = in-place).
    let shrunk = unsafe { crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, 16) };
    assert_eq!(
        shrunk, ptr,
        "shrink to 50% must be in-place"
    );

    // Grow back to original size — should also be in-place since 32 fits the class.
    let grew = unsafe { crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, 32) };
    assert_eq!(
        grew, ptr,
        "grow back to original must be in-place"
    );

    unsafe { crate::thread_free::<StandardPolicy, DefaultBackend>(ptr) };
}

/// Realloc multiple times in a loop to stress the in-place reuse path.
#[test]
fn realloc_repeated_grow_shrink_cycle() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let ptr = alloc_small(&mut alloc, 8);
    let mut current_size = 8usize;

    // Grow and shrink repeatedly, staying within the same size class.
    let stride = block_stride_for(ptr);
    for _ in 0..20 {
        // Grow to just under the stride.
        let target = stride - 8;
        let layout = core::alloc::Layout::from_size_align(current_size, 8).expect("layout valid");
        let result = unsafe {
            crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, target)
        };
        assert_eq!(
            result, ptr,
            "in-place grow failed during cycle (current={current_size}, target={target})"
        );
        current_size = target;

        // Shrink back to 50%.
        let half = current_size / 2;
        let layout = core::alloc::Layout::from_size_align(current_size, 8).expect("layout valid");
        let result = unsafe {
            crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, half)
        };
        assert_eq!(
            result, ptr,
            "in-place shrink failed during cycle (current={current_size}, target={half})"
        );
        current_size = half;
    }

    unsafe { crate::thread_free::<StandardPolicy, DefaultBackend>(ptr) };
}

/// Realloc across multiple threads: one thread reallocs, another frees the result.
///
/// Exercises the cross-thread ownership transfer after realloc:
/// thread A reallocs (producing a new block), thread B frees it through
/// the cross-thread free path.
#[test]
fn realloc_cross_thread_free_of_result() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let ptr = alloc_small(&mut alloc, 32);
    let layout = core::alloc::Layout::from_size_align(32, 8).expect("layout valid");

    // Grow to a different class — forces a new block allocation.
    let result = unsafe { crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, 128) };
    assert!(!result.is_null());
    assert_ne!(result, ptr);

    let result_val = result as usize;

    // realloc already freed ptr internally. Only free result from a different thread.
    let handle = std::thread::spawn(move || unsafe {
        crate::thread_free::<StandardPolicy, DefaultBackend>(result_val as *mut u8);
    });
    handle.join().expect("cross-thread free of realloc result panicked");
}

/// Realloc within the same class preserves a multi-byte marker pattern.
#[test]
fn realloc_within_class_preserves_full_pattern() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let ptr = alloc_small(&mut alloc, 32);

    // Fill with a distinct pattern.
    unsafe {
        for i in 0..32 {
            *ptr.add(i) = i as u8;
        }
    }

    let stride = block_stride_for(ptr);
    let layout = core::alloc::Layout::from_size_align(32, 8).expect("layout valid");

    // Grow within the class.
    let result = unsafe { crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, stride - 1) };
    assert_eq!(result, ptr, "grow within class must be in-place");

    // Verify all 32 bytes preserved.
    for i in 0..32 {
        let expected = i as u8;
        let actual = unsafe { *ptr.add(i) };
        assert_eq!(
            actual, expected,
            "byte {i} corrupted after in-place grow: expected {expected:#x}, got {actual:#x}"
        );
    }

    unsafe { crate::thread_free::<StandardPolicy, DefaultBackend>(ptr) };
}

/// Realloc grow beyond class copies data correctly when old < new.
#[test]
fn realloc_grow_beyond_class_copies_full_old_data() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let ptr = alloc_small(&mut alloc, 8);

    // Fill with pattern.
    unsafe {
        for i in 0..8 {
            *ptr.add(i) = 0xAA + i as u8;
        }
    }

    let stride = block_stride_for(ptr);
    let new_size = stride + 1;
    let layout = core::alloc::Layout::from_size_align(8, 8).expect("layout valid");
    let result = unsafe { crate::thread_realloc::<StandardPolicy, DefaultBackend>(ptr, layout, new_size) };
    assert!(!result.is_null());
    assert_ne!(result, ptr);

    // Verify the original 8 bytes.
    for i in 0..8 {
        let expected = 0xAA + i as u8;
        let actual = unsafe { *result.add(i) };
        assert_eq!(
            actual, expected,
            "byte {i} mismatch: expected {expected:#x}, got {actual:#x}"
        );
    }

    // realloc already freed ptr internally.
    unsafe { crate::thread_free::<StandardPolicy, DefaultBackend>(result) };
}
