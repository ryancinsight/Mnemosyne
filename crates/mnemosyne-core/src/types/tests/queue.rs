//! The cross-thread free queue: ordering, its exact count, and the
//! duplicate-push abort.

use super::*;
use crate::types::{Page, Segment};
use ::std::{
    alloc::{alloc_zeroed, dealloc},
    string::String,
};

#[test]
fn atomic_free_list_standard_mode_keeps_lifo_order_and_exact_count() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(!segment_ptr.is_null(), "segment allocation failed");
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };

    const PAGE_INDEX: usize = 1;
    let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
    unsafe {
        (*page).block_size = 16;
        (*page).size_class = 0;
    }

    let page_start = unsafe { Page::page_start_in_segment(segment_ptr, PAGE_INDEX) };
    unsafe {
        Page::initialize_free_list_in_segment::<crate::policy::StandardPolicy>(
            segment_ptr,
            PAGE_INDEX,
            page_start,
            0,
        );
    }

    let block_a = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };
    let block_b = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };
    let block_c = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };
    let queue = unsafe { &(*page).thread_free };

    queue.push_raw(block_a);
    queue.push_raw(block_b);
    queue.push_raw(block_c);

    let (head, count) = queue.pop_all_raw().expect("queue must contain 3 blocks");
    assert_eq!(
        count, 3,
        "standard-mode count must equal detached chain length"
    );
    assert_eq!(head, block_c, "last push must become the new head");
    assert_eq!(unsafe { (*head.as_ptr()).get_next_raw() }, Some(block_b));
    assert_eq!(unsafe { (*block_b.as_ptr()).get_next_raw() }, Some(block_a));
    assert_eq!(unsafe { (*block_a.as_ptr()).get_next_raw() }, None);

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}
#[test]
fn atomic_free_list_encrypted_mode_keeps_lifo_order_and_exact_count() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(!segment_ptr.is_null(), "segment allocation failed");
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };
    unsafe { (*segment_ptr).free_list_encrypted = true };

    const PAGE_INDEX: usize = 1;
    let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
    unsafe {
        (*page).block_size = 16;
        (*page).size_class = 0;
    }

    let page_start = unsafe { Page::page_start_in_segment(segment_ptr, PAGE_INDEX) };
    unsafe {
        Page::initialize_free_list_in_segment::<crate::policy::HardenedPolicy>(
            segment_ptr,
            PAGE_INDEX,
            page_start,
            0,
        );
    }

    let block_a = unsafe { Page::pop_block::<crate::policy::HardenedPolicy>(page) };
    let block_b = unsafe { Page::pop_block::<crate::policy::HardenedPolicy>(page) };
    let cookie =
        unsafe { Segment::cookie_for::<crate::policy::HardenedPolicy>(segment_ptr, PAGE_INDEX) };
    let queue = unsafe { &(*page).thread_free };

    queue.push_dynamic(block_a, true);
    queue.push_dynamic(block_b, true);

    let (head, count) = queue
        .pop_all(true, cookie)
        .expect("queue must contain 2 blocks");
    assert_eq!(
        count, 2,
        "encrypted-mode count must equal detached chain length"
    );
    assert_eq!(head, block_b, "last push must become the new head");
    assert_eq!(
        unsafe { (*head.as_ptr()).get_next_dynamic(true, cookie) },
        Some(block_a)
    );
    assert_eq!(
        unsafe { (*block_a.as_ptr()).get_next_dynamic(true, cookie) },
        None
    );

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}
#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn atomic_free_list_rejects_duplicate_push_in_standard_mode() {
    if std::env::var_os("MNEMOSYNE_ATOMIC_FREE_LIST_DUPLICATE_GUARD").is_some() {
        let layout = segment_layout();
        let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
        assert!(!segment_ptr.is_null(), "segment allocation failed");
        unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };

        const PAGE_INDEX: usize = 1;
        let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
        unsafe {
            (*page).block_size = 16;
            (*page).size_class = 0;
        }

        let page_start = unsafe { Page::page_start_in_segment(segment_ptr, PAGE_INDEX) };
        unsafe {
            Page::initialize_free_list_in_segment::<crate::policy::StandardPolicy>(
                segment_ptr,
                PAGE_INDEX,
                page_start,
                0,
            );
        }

        let block = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };
        let queue = unsafe { &(*page).thread_free };
        queue.push_raw(block);
        queue.push_raw(block);
        panic!("standard-mode duplicate push should abort after the second enqueue");
    }

    let output = std::process::Command::new(
        std::env::current_exe().expect("invariant: a test binary knows its own path"),
    )
    .env("MNEMOSYNE_ATOMIC_FREE_LIST_DUPLICATE_GUARD", "1")
    .arg("atomic_free_list_rejects_duplicate_push_in_standard_mode")
    .arg("--nocapture")
    .output()
    .expect("child test process should run");

    assert!(
        !output.status.success(),
        "standard-mode duplicate push must abort; child stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn atomic_free_list_rejects_duplicate_push_in_encrypted_mode() {
    if std::env::var_os("MNEMOSYNE_ATOMIC_FREE_LIST_DUPLICATE_GUARD").is_some() {
        let layout = segment_layout();
        let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
        assert!(!segment_ptr.is_null(), "segment allocation failed");
        unsafe {
            Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0);
            (*segment_ptr).free_list_encrypted = true;
        }

        const PAGE_INDEX: usize = 1;
        let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
        unsafe {
            (*page).block_size = 16;
            (*page).size_class = 0;
        }

        let page_start = unsafe { Page::page_start_in_segment(segment_ptr, PAGE_INDEX) };
        unsafe {
            Page::initialize_free_list_in_segment::<crate::policy::HardenedPolicy>(
                segment_ptr,
                PAGE_INDEX,
                page_start,
                0,
            );
        }

        let block = unsafe { Page::pop_block::<crate::policy::HardenedPolicy>(page) };
        let queue = unsafe { &(*page).thread_free };
        queue.push_dynamic(block, true);
        queue.push_dynamic(block, true);
        panic!("encrypted-mode duplicate push should abort after the second enqueue");
    }

    let output = std::process::Command::new(
        std::env::current_exe().expect("invariant: a test binary knows its own path"),
    )
    .env("MNEMOSYNE_ATOMIC_FREE_LIST_DUPLICATE_GUARD", "1")
    .arg("atomic_free_list_rejects_duplicate_push_in_encrypted_mode")
    .arg("--nocapture")
    .output()
    .expect("child test process should run");

    assert!(
        !output.status.success(),
        "encrypted-mode duplicate push must abort; child stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
