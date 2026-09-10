//! The backward-edge free canary and the segment cookie it is bound to.

use super::*;
use crate::types::Segment;
use ::std::alloc::{Layout, alloc_zeroed, dealloc};

#[test]
fn free_canary_write_check_clear_roundtrip() {
    use crate::constants::MIN_BLOCK_SIZE;
    use crate::types::Block;

    // Allocate a block-sized buffer with the minimum block size.
    let layout = Layout::from_size_align(MIN_BLOCK_SIZE, MIN_BLOCK_SIZE).expect("valid layout");
    let ptr = unsafe { alloc_zeroed(layout) } as *mut Block;
    assert!(!ptr.is_null());

    // Keep the fixture within the wasm32 pointer width so the same canary
    // contract is exercised on every supported target.
    let page_cookie: usize = 0x1234_5678;

    // Initially no canary -- check_double_free should return false.
    let has_canary = unsafe { Block::check_double_free(ptr, page_cookie) };
    assert!(!has_canary, "fresh allocation must not carry a canary");

    // Write the canary.
    unsafe { Block::write_free_canary(ptr, page_cookie) };

    // Now check_double_free should return true.
    let has_canary = unsafe { Block::check_double_free(ptr, page_cookie) };
    assert!(
        has_canary,
        "canary must be detected after write_free_canary"
    );

    // A different cookie must NOT match -- the canary is address+cookie bound.
    let wrong_cookie: usize = 0x3333_4444;
    let wrong_match = unsafe { Block::check_double_free(ptr, wrong_cookie) };
    assert!(
        !wrong_match,
        "canary must not match a different page cookie"
    );

    // Clear the canary.
    unsafe { Block::clear_free_canary(ptr) };
    let has_canary = unsafe { Block::check_double_free(ptr, page_cookie) };
    assert!(!has_canary, "canary must be gone after clear_free_canary");

    unsafe { dealloc(ptr as *mut u8, layout) };
}
#[test]
fn free_canary_is_address_bound() {
    use crate::constants::MIN_BLOCK_SIZE;
    use crate::types::Block;

    // Two adjacent blocks with the same page_cookie must produce different canaries.
    let layout = Layout::from_size_align(MIN_BLOCK_SIZE * 2, MIN_BLOCK_SIZE).expect("valid layout");
    let base = unsafe { alloc_zeroed(layout) } as *mut Block;
    assert!(!base.is_null());

    // SAFETY: both pointers are within the MIN_BLOCK_SIZE * 2 allocation.
    let block_a = base;
    let block_b = unsafe { base.add(1) }; // MIN_BLOCK_SIZE offset

    let cookie: usize = 0xCAFE_0001;

    unsafe { Block::write_free_canary(block_a, cookie) };
    unsafe { Block::write_free_canary(block_b, cookie) };

    // block_b's canary must not match block_a's slot.
    let a_canary = unsafe { block_a.cast::<usize>().add(1).read() };
    let b_canary = unsafe { block_b.cast::<usize>().add(1).read() };
    assert_ne!(
        a_canary, b_canary,
        "adjacent blocks with the same cookie must have different canaries (address binding)"
    );

    unsafe { dealloc(base as *mut u8, layout) };
}
#[test]
fn segment_cookie_for_hardened_policy_uses_page_key() {
    let layout = segment_layout();
    let segment = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(!segment.is_null());

    unsafe { Segment::initialize(segment, segment as *mut u8, 0) };
    unsafe { (*segment).free_list_encrypted = true };

    let page_index = 1;
    let cookie =
        unsafe { Segment::cookie_for::<crate::policy::HardenedPolicy>(segment, page_index) };
    assert_eq!(
        cookie,
        unsafe { (*segment).keys[page_index] },
        "HardenedPolicy must derive the free-list cookie from the page key"
    );

    unsafe { dealloc(segment as *mut u8, layout) };
}
