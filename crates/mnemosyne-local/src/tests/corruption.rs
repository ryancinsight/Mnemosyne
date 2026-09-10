//! Metadata and free-list corruption detection at the abort boundary.

use super::*;

/// A blind write into a freed hardened block must not survive the next
/// allocation from that page.
///
/// The earlier form of this test flipped a bit, read the link back, observed
/// that it decoded to something else, and then repaired it -- which proves
/// only that the encoding is an encoding, since any XOR decodes a flipped
/// word differently. Nothing allocated through the tampered chain, so the
/// check the test is named for never ran. The abort is the observable, so
/// the tamper runs in a re-executed child like the three cases below.
///
/// The encoding is `addr ^ (cookie | 1)`, so flipping bit `SEGMENT_SIZE` of
/// the encoded word moves the decoded address exactly one segment away while
/// leaving its alignment intact: the attacker needs no knowledge of the
/// cookie, and `pop_block`'s in-page bound is what has to reject it. A
/// tamper that merely misaligned the link would be caught by the cheaper
/// alignment test instead.
#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn hardened_policy_detects_freelist_tamper() {
    use mnemosyne_core::policy::HardenedPolicy;
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_HARDENED_FREELIST_TAMPER_ABORT_TEST").is_ok() {
        let _guard = crate::local_alloc::TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");

        // Two blocks of class 0 (16 bytes) allocated in sequence share a page,
        // and freeing both leaves a head with a real next-link to tamper with.
        let ptr1 = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(16, 8) };
        let ptr2 = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(16, 8) };
        assert!(!ptr1.is_null());
        assert!(!ptr2.is_null());
        unsafe {
            thread_free::<HardenedPolicy, MemoryBackendWrapper>(ptr1);
            thread_free::<HardenedPolicy, MemoryBackendWrapper>(ptr2);
        }

        let ptr_val = ptr1 as usize;
        let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
        let page = unsafe { &raw mut (*segment).pages[page_index] };

        // The randomized head (ADR 0001, revised 2026-09-09) decides which of
        // the two chains holds the block the next allocation pops, so the
        // tamper follows the head rather than naming an address.
        let active_head = unsafe { (*page).free.or((*page).secondary_free) }
            .expect("invariant: two frees onto one page leave a live head in one of its chains");
        let raw_word = active_head.as_ptr() as *mut usize;

        // SAFETY: `raw_word` is the head block's own first word, which holds
        // the encoded link; the block is free, so no live allocation aliases
        // it. This is the whole tamper -- no cookie is read.
        unsafe {
            *raw_word ^= SEGMENT_SIZE;
        }

        // Allocating from this page consults the free chain (the bump region
        // is skipped while either chain is non-empty) and decodes the tampered
        // link, which lands one segment outside the page.
        let _ = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(16, 8) };
        return;
    }

    let current_exe = env::current_exe().expect("invariant: a running test binary has a path");
    let output = Command::new(current_exe)
        .arg("tests::corruption::hardened_policy_detects_freelist_tamper")
        .arg("--exact")
        .env("RUN_HARDENED_FREELIST_TAMPER_ABORT_TEST", "1")
        .output()
        .expect("invariant: re-executing this test binary succeeds");

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("allocating through a tampered hardened free-list link must abort the process");
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
