use crate::types::{Block, Page, Segment};
use ::std::{
    alloc::{Layout, alloc_zeroed, dealloc},
    string::String,
};
use core::ptr::NonNull;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RandomizedTestPolicy;

impl crate::policy::private::Sealed for RandomizedTestPolicy {}
impl crate::policy::AllocPolicy for RandomizedTestPolicy {
    const ENABLE_POISONING: bool = false;
    const ZERO_INITIALIZE: bool = false;
    const RANDOMIZE_ALLOCATION: bool = true;
}

fn segment_layout() -> Layout {
    Layout::from_size_align(
        crate::constants::SEGMENT_SIZE,
        crate::constants::SEGMENT_SIZE,
    )
    .expect("segment layout uses equal power-of-two size and alignment")
}

#[test]
fn page_struct_size_stays_within_one_cache_line() {
    // Page metadata is hot: every allocation reads and writes
    // `page.free`, `page.alloc_count`, and `page.block_size`. Keeping
    // the struct within a single 64-byte cache line on 64-bit targets
    // ensures the fast path touches only one cache line per page
    // operation.
    assert!(
        core::mem::size_of::<Page>() <= 64,
        "Page exceeds one 64-byte cache line ({} bytes)",
        core::mem::size_of::<Page>()
    );
}

#[test]
fn segment_default_free_list_mode_matches_standard_runtime_state() {
    let segment = Segment::default();
    assert!(!segment.free_list_encrypted);
    assert!(unsafe { Segment::free_list_mode_matches(&segment, false) });
    assert!(!unsafe { Segment::free_list_mode_matches(&segment, true) });
}

#[test]
fn free_list_mode_matches_segment_state() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };

    assert!(unsafe { Segment::free_list_mode_matches(segment_ptr, false) });
    assert!(!unsafe { Segment::free_list_mode_matches(segment_ptr, true) });

    unsafe { (*segment_ptr).free_list_encrypted = true };
    assert!(unsafe { Segment::free_list_mode_matches(segment_ptr, true) });
    assert!(!unsafe { Segment::free_list_mode_matches(segment_ptr, false) });

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}

#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn reclaim_thread_free_if_present_rejects_mismatched_mode() {
    if std::env::var_os("MNEMOSYNE_RECLAIM_MODE_GUARD").is_some() {
        let layout = segment_layout();
        let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
        assert!(
            !segment_ptr.is_null(),
            "alloc_zeroed failed to allocate segment"
        );
        unsafe {
            Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0);
            (*segment_ptr).free_list_encrypted = true;
            Page::reclaim_thread_free_if_present_in_segment(segment_ptr, 1, false);
        }
        panic!("raw reclaim path should have aborted on free-list mode mismatch");
    }

    let output = std::process::Command::new(
        std::env::current_exe().expect("invariant: a test binary knows its own path"),
    )
    .env("MNEMOSYNE_RECLAIM_MODE_GUARD", "1")
    .arg("reclaim_thread_free_if_present_rejects_mismatched_mode")
    .arg("--nocapture")
    .output()
    .expect("child test process should run");

    assert!(
        !output.status.success(),
        "raw reclaim path must abort on a mode mismatch; child stderr: {}",
        std::string::String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_page_reclaim_thread_free() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };
    // The page is addressed through `segment_ptr` for the whole test rather
    // than through a long-lived `&mut Page`. Pushing to `thread_free` and
    // reclaiming both reach the segment header (for the free-list cookie), and
    // a page borrow held across those calls sits on a different provenance than
    // the segment access, which invalidates it. In production those pushes come
    // from a *remote* thread that holds no page borrow at all, so addressing by
    // segment is also the faithful shape.
    const PAGE_INDEX: usize = 1;
    let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
    unsafe { (*page).block_size = 16 };

    unsafe {
        let page_start = Page::page_start_in_segment(segment_ptr, PAGE_INDEX);
        Page::initialize_free_list_in_segment::<crate::policy::StandardPolicy>(
            segment_ptr,
            PAGE_INDEX,
            page_start,
            0,
        );
    }

    let first = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };
    unsafe { (*page).alloc_count = 1 };
    unsafe {
        (*page)
            .thread_free
            .push::<crate::policy::StandardPolicy>(first)
    };

    let reclaimed =
        unsafe { Page::reclaim_thread_free_if_present_in_segment(segment_ptr, PAGE_INDEX, false) };

    assert_eq!(reclaimed, 1);
    assert_eq!(unsafe { (*page).alloc_count }, 0);
    assert_eq!(unsafe { (*page).free }, Some(first));
    assert!(
        unsafe { (*page).thread_free.is_empty() },
        "thread_free list was not empty after reclaim"
    );

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}

#[test]
fn test_page_reclaim_thread_free_hot_path() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };
    // Addressed through `segment_ptr` throughout: pushing to `thread_free` and
    // reclaiming both read the segment header for the free-list cookie, and a
    // `&mut Page` held across those accesses is invalidated by them — Tree
    // Borrows disables the page tag, Stacked Borrows pops it.
    const PAGE_INDEX: usize = 1;
    let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
    unsafe { (*page).block_size = 16 };

    unsafe {
        let page_start = Page::page_start_in_segment(segment_ptr, PAGE_INDEX);
        Page::initialize_free_list_in_segment::<crate::policy::StandardPolicy>(
            segment_ptr,
            PAGE_INDEX,
            page_start,
            0,
        );
    }

    let b1 = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };
    let b2 = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };

    // Simulate all other blocks allocated / empty free list
    unsafe {
        (*page).free = None;
        (*page).alloc_count = 2;
        (*page)
            .thread_free
            .push::<crate::policy::StandardPolicy>(b1);
        (*page)
            .thread_free
            .push::<crate::policy::StandardPolicy>(b2);
    }

    // Reclaim thread_free. Since page.free is None, this triggers O(1) swap.
    let reclaimed = unsafe {
        Page::reclaim_thread_free_for_policy::<crate::policy::StandardPolicy>(
            segment_ptr,
            PAGE_INDEX,
        )
    };

    assert_eq!(reclaimed, 2);
    assert_eq!(unsafe { (*page).alloc_count }, 0);
    assert_eq!(unsafe { (*page).free }, Some(b2));

    unsafe {
        let next_node = (*b2.as_ptr()).get_next::<crate::policy::StandardPolicy>(0);
        assert_eq!(next_node, Some(b1));
        assert_eq!(
            (*b1.as_ptr()).get_next::<crate::policy::StandardPolicy>(0),
            None
        );
    }
    assert!(
        unsafe { (*page).thread_free.is_empty() },
        "thread_free list was not empty after reclaim"
    );

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}

#[test]
fn page_wake_hysteresis_uses_single_threshold_source() {
    let class = 0;
    let denom = 4;
    let max_blocks = crate::size_class::class_to_max_blocks(class);
    let threshold = Page::wake_threshold_for_class(class, denom);

    assert_eq!(threshold, max_blocks / denom.max(1));
    assert!(!Page::should_reactivate_after_free(
        class,
        max_blocks - 1,
        denom
    ));
    assert!(Page::should_reactivate_after_free(
        class,
        max_blocks - threshold,
        denom
    ));
}

#[test]
fn randomized_page_free_list_uses_seeded_permutation() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };
    // Addressed through the segment: free-list initialization reads the
    // segment's cookie, and a page borrow held across that access is
    // invalidated by it.
    const PAGE_INDEX: usize = 1;
    let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
    unsafe {
        (*page).block_size = 16;
        (*page).size_class = 0;
    }

    unsafe {
        let page_start = Page::page_start_in_segment(segment_ptr, PAGE_INDEX);
        Page::initialize_free_list_in_segment::<RandomizedTestPolicy>(
            segment_ptr,
            PAGE_INDEX,
            page_start,
            (7 << 16) | 5,
        );

        let primary = (*page).free;
        let secondary = (*page).secondary_free;
        assert!(
            primary.is_some(),
            "randomized page must keep a primary free-list head"
        );
        assert!(
            secondary.is_some(),
            "randomized page must keep a secondary free-list head"
        );

        let expected_head = if Page::prefer_secondary_free(page, (*page).alloc_count as usize) {
            secondary
        } else {
            primary
        };
        let first = Page::pop_block::<RandomizedTestPolicy>(page);
        assert_eq!(
            Some(first),
            expected_head,
            "the seeded random policy must choose the active free-list head"
        );

        let cookie = Segment::cookie_for::<RandomizedTestPolicy>(segment_ptr, PAGE_INDEX);
        let second = Page::pop_block::<RandomizedTestPolicy>(page);
        let expected_second = (*first.as_ptr()).get_next::<RandomizedTestPolicy>(cookie);
        assert_eq!(
            Some(second),
            expected_second,
            "the second pop must follow the active list's next link"
        );

        dealloc(segment_ptr as *mut u8, layout);
    }
}

#[test]
fn reclaim_if_present_for_policy_keeps_randomized_head_selection() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };

    const PAGE_INDEX: usize = 1;
    let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
    let page_start = unsafe { Page::page_start_in_segment(segment_ptr, PAGE_INDEX) };
    unsafe {
        (*page).block_size = 16;
        (*page).size_class = 0;
        (*page).alloc_count = 1;
        (*page).free = None;
        (*page).secondary_free = None;
    }

    let block_a = unsafe { NonNull::new_unchecked(page_start.add(0) as *mut Block) };
    let block_b = unsafe { NonNull::new_unchecked(page_start.add(16) as *mut Block) };
    let remote = unsafe { NonNull::new_unchecked(page_start.add(32) as *mut Block) };
    unsafe {
        (*block_a.as_ptr()).set_next::<RandomizedTestPolicy>(None, 0);
        (*block_b.as_ptr()).set_next::<RandomizedTestPolicy>(None, 0);
        (*page).secondary_free = Some(block_a);
        (*page).free = Some(block_b);
        (*page).thread_free.push::<RandomizedTestPolicy>(remote);
    }

    let reclaimed = unsafe {
        Page::reclaim_thread_free_if_present_for_policy::<RandomizedTestPolicy>(
            segment_ptr,
            PAGE_INDEX,
        )
    };
    assert_eq!(
        reclaimed, 1,
        "the remote-free drain must reclaim the queued block"
    );

    let (expected_head, _expected_secondary) =
        unsafe { Page::choose_free_head(page, (*page).alloc_count as usize, true) };
    let active_head = unsafe {
        if (*page).secondary_free.is_some() && (*page).free.is_some() {
            if Page::prefer_secondary_free(page, (*page).alloc_count as usize) {
                (*page).secondary_free
            } else {
                (*page).free
            }
        } else {
            (*page).secondary_free.or((*page).free)
        }
    };
    assert_eq!(
        active_head, expected_head,
        "randomized remote-free drain must preserve the active free-list head"
    );

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}

#[test]
fn standard_policy_keeps_secondary_free_list_active_when_present() {
    let layout = segment_layout();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    unsafe { Segment::initialize(segment_ptr, segment_ptr as *mut u8, 0) };

    const PAGE_INDEX: usize = 1;
    let page = unsafe { &raw mut (*segment_ptr).pages[PAGE_INDEX] };
    let page_start = unsafe { Page::page_start_in_segment(segment_ptr, PAGE_INDEX) };
    unsafe {
        (*page).block_size = 16;
        (*page).size_class = 0;
        (*page).free = None;
        (*page).secondary_free = None;
    }

    let block_a = unsafe { NonNull::new_unchecked(page_start.add(0) as *mut Block) };
    let block_b = unsafe { NonNull::new_unchecked(page_start.add(16) as *mut Block) };
    unsafe {
        (*block_a.as_ptr()).set_next::<crate::policy::StandardPolicy>(Some(block_b), 0);
        (*block_b.as_ptr()).set_next::<crate::policy::StandardPolicy>(None, 0);
        (*page).secondary_free = Some(block_a);
    }

    let first = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };
    assert_eq!(
        Some(first),
        Some(block_a),
        "standard policy must pop the active secondary list head"
    );

    let second = unsafe { Page::pop_block::<crate::policy::StandardPolicy>(page) };
    assert_eq!(
        Some(second),
        Some(block_b),
        "standard policy must preserve the secondary list order"
    );

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}

#[test]
fn huge_mapping_suffix_uses_raw_mapping_base() {
    let mut segment_storage = core::mem::MaybeUninit::<Segment>::uninit();
    let segment = segment_storage.as_mut_ptr();
    let mut mapping = std::vec![0_u8; 0x4000];
    let raw = mapping.as_mut_ptr();
    unsafe {
        Segment::initialize(segment, raw, 0);
        (*segment).pages[0].block_size = 0x4000;
    }

    let expected_key =
        (segment as usize).wrapping_add(crate::constants::PAGE_SIZE) ^ (usize::MAX / 3);
    assert_eq!(
        unsafe { (*segment).keys[1] },
        expected_key,
        "segment page keys must use the pointer-width mask"
    );

    let user_ptr = unsafe { raw.add(0x1800) }.cast_const();
    let suffix = unsafe { (*segment).huge_mapping_suffix_from(user_ptr) };

    assert_eq!(
        suffix, 0x2800,
        "huge usable suffix must be raw_alloc_ptr + block_size - user_ptr"
    );
}

#[test]
fn huge_mapping_suffix_is_address_space_safe() {
    let mut segment_storage = core::mem::MaybeUninit::<Segment>::uninit();
    let segment = segment_storage.as_mut_ptr();
    // The case is about arithmetic at the top of the address space, so the two
    // pointers are synthesized at their addresses rather than offset from one
    // another: `add` on a pointer with no allocation behind it is UB in its own
    // right, which is what Miri reported here, and it is not the property under
    // test. `huge_mapping_suffix_from` only reads `.addr()`, so neither pointer
    // is ever dereferenced.
    let base = usize::MAX - 0x4000;
    let raw = core::ptr::without_provenance_mut::<u8>(base);
    unsafe {
        Segment::initialize(segment, raw, 0);
        (*segment).pages[0].block_size = 0x4000;
    }

    let user_ptr = core::ptr::without_provenance::<u8>(base + 0x1800);
    let suffix = unsafe { (*segment).huge_mapping_suffix_from(user_ptr) };

    assert_eq!(
        suffix, 0x2800,
        "huge suffix must avoid wrapping near the top of the address space"
    );
}
// -- Backward-edge free canary tests ------------------------------------------

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
