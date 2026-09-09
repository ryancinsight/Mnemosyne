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

    // The randomized head (ADR 0001, revised 2026-09-09) makes "which block is
    // reused" unpredictable, so the tamper is applied to whichever block is
    // actually at the head and the claim is made about decoding rather than
    // about an address. Reading the link back under the same cookie must not
    // reproduce what was there before the flip; if it did, the encoding would
    // be carrying tampered bits through faithfully.
    let ptr_val = ptr1 as usize;
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    let page = unsafe { &raw mut (*segment).pages[page_index] };

    let active_head = unsafe { (*page).free.or((*page).secondary_free) }
        .expect("HardenedPolicy should keep at least one free block live after the paired free");
    let cookie = unsafe { Segment::cookie_for::<HardenedPolicy>(segment, page_index) };
    let expected_next = unsafe { (*active_head.as_ptr()).get_next::<HardenedPolicy>(cookie) };
    let raw_word = active_head.as_ptr() as *mut usize;

    // SAFETY: `raw_word` is the head block's own first word, which holds the
    // encoded link; the block is free, so no live allocation aliases it.
    unsafe {
        let original = *raw_word;
        *raw_word = original ^ 0x08;
    }

    // SAFETY: same block, same cookie -- this reads the link the flip left.
    let tampered_next = unsafe { (*active_head.as_ptr()).get_next::<HardenedPolicy>(cookie) };
    assert_ne!(
        tampered_next, expected_next,
        "HardenedPolicy must not faithfully decode a tampered free-list link"
    );

    // Restore the link so the page returns to a consistent state and its
    // segment reclaims normally; every observation above was made against the
    // tampered state, so the repair changes nothing the test measures.
    // SAFETY: same block and cookie, writing back the value read before the flip.
    unsafe {
        (*active_head.as_ptr()).set_next::<HardenedPolicy>(expected_next, cookie);
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
