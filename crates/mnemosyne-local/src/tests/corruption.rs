//! Metadata and free-list corruption detection at the abort boundary.

use super::*;

#[test]
fn hardened_policy_detects_freelist_tamper() {
    use mnemosyne_core::policy::HardenedPolicy;

    let _guard = crate::local_alloc::TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    // We want to verify that tamper detection works under HardenedPolicy.
    // Let's allocate two blocks on a fresh page of class 0 (16 bytes).
    // Since we want them on the same page, we can allocate them in sequence.
    let ptr1 = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(16, 8) };
    let ptr2 = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(16, 8) };
    assert!(!ptr1.is_null());
    assert!(!ptr2.is_null());

    // Free them in sequence so they end up in the thread-local free list
    unsafe {
        thread_free::<HardenedPolicy, MemoryBackendWrapper>(ptr1);
        thread_free::<HardenedPolicy, MemoryBackendWrapper>(ptr2);
    }

    // Now, `page.free` points to `ptr2`, and `ptr2` contains the encrypted pointer to `ptr1`.
    // Let's tamper with the encrypted next pointer in `ptr2`.
    // The block metadata stores the encrypted pointer in the first `Option<NonNull<Block>>` slot of the block.
    let val2 = ptr2 as *mut usize;
    let original_val = unsafe { *val2 };
    unsafe {
        // Corrupt the pointer (e.g. flip a bit in the address portion)
        *val2 = original_val ^ 0x08;
    }

    // Now, try to allocate. The first allocation gets `ptr2` (which is successful).
    let ptr3 = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(16, 8) };
    assert_eq!(ptr3, ptr2);

    // The second allocation would follow the tampered pointer to `ptr1`.
    // Since we flipped a bit, the decrypted address is incorrect and fails to match `ptr1`.
    // In particular, the page's free pointer now contains garbage.
    let ptr_val = ptr3 as usize;
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    let page = unsafe { (*segment).pages.get_unchecked(page_index) };

    let free_head = page.free.map(|p| p.as_ptr() as usize);
    assert_ne!(
        free_head,
        Some(ptr1 as usize),
        "HardenedPolicy failed to obscure/randomize the tampered pointer"
    );

    // The claim is proven; now put the page back in a consistent state. The
    // tampering left `page.free` holding a decrypted garbage address, so the
    // allocator can neither hand out another block from this page nor reclaim
    // its segment, and the segment would still be held at process exit — which
    // Miri reports as a leak. Clearing the head restores an empty free list,
    // after which freeing the outstanding block lets the ordinary reclaim path
    // release the segment like any other test's.
    //
    // Repairing after the assertion changes nothing the test measures: every
    // observation above has already been made against the tampered state.
    // SAFETY: `segment`/`page_index` locate this thread's own live page, and
    // the raw place projection avoids retagging the enclosing segment.
    unsafe {
        let page = &raw mut (*segment).pages[page_index];
        (*page).free = None;
        thread_free::<HardenedPolicy, MemoryBackendWrapper>(ptr3);
    }
}

#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn test_large_alloc_metadata_corruption_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_LARGE_ALLOC_METADATA_CORRUPTION_ABORT_TEST").is_ok() {
        unsafe {
            let ptr = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(65536, 8);
            assert!(!ptr.is_null());

            // Corrupt the metadata slot immediately preceding the payload.
            let metadata_slot = (ptr as *mut usize).sub(1);
            metadata_slot.write(0x1337); // Invalid segment pointer alignment.

            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::corruption::test_large_alloc_metadata_corruption_aborts_process")
        .arg("--exact")
        .env("RUN_LARGE_ALLOC_METADATA_CORRUPTION_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn test_large_alloc_segment_invariant_corruption_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_LARGE_ALLOC_SEGMENT_INVARIANT_CORRUPTION_ABORT_TEST").is_ok() {
        unsafe {
            let ptr = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(65536, 8);
            assert!(!ptr.is_null());

            // Retrieve the valid segment pointer from the metadata slot.
            let segment_ptr = *((ptr as *mut *mut Segment).sub(1));

            // Corrupt the segment header's raw_alloc_ptr to violate the alignment offset invariant.
            (*segment_ptr).raw_alloc_ptr = core::ptr::null_mut();

            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::corruption::test_large_alloc_segment_invariant_corruption_aborts_process")
        .arg("--exact")
        .env(
            "RUN_LARGE_ALLOC_SEGMENT_INVARIANT_CORRUPTION_ABORT_TEST",
            "1",
        )
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn test_free_list_corruption_out_of_bounds_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_FREE_LIST_CORRUPTION_OUT_OF_BOUNDS_ABORT_TEST").is_ok() {
        unsafe {
            // Allocate a small block
            let ptr = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            assert!(!ptr.is_null());

            // Get containing segment and page
            let ptr_val = ptr as usize;
            let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;
            let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

            // Free the block to put it on the free list
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);

            // Corrupt the next pointer inside the block.
            // Since it's on page.free, we can decrypt/encrypt the next pointer.
            // Let's write an out-of-bounds address (e.g., 0x12345678) as the next pointer.
            let cookie = (*segment).keys[page_index];
            let corrupt_block = ptr as *mut Block;
            // Write corrupted next pointer
            let bad_ptr = 0x12345678 as *mut Block;
            (*corrupt_block).set_next::<StandardPolicy>(NonNull::new(bad_ptr), cookie);

            // Allocate again. This should pop the corrupted next pointer, but
            // when pop_block retrieves it from page.free, it will validate it and abort!
            let _ptr_new = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            let _ptr_another = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::corruption::test_free_list_corruption_out_of_bounds_aborts_process")
        .arg("--exact")
        .env("RUN_FREE_LIST_CORRUPTION_OUT_OF_BOUNDS_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}
