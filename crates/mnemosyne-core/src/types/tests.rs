use crate::types::{Page, Segment};
use ::std::alloc::{Layout, alloc_zeroed, dealloc};

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

        let first = Page::pop_block::<RandomizedTestPolicy>(page);
        let second = Page::pop_block::<RandomizedTestPolicy>(page);
        let block_size = (*page).block_size;

        assert_eq!(
            first.as_ptr() as usize - page_start as usize,
            7 * block_size,
            "randomized free list must start at the seed-derived index"
        );
        assert_eq!(
            second.as_ptr() as usize - page_start as usize,
            12 * block_size,
            "randomized free list must advance by the seed-derived coprime stride"
        );

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

    let user_ptr = unsafe { raw.add(0x1800) }.cast_const();
    let suffix = unsafe { (*segment).huge_mapping_suffix_from(user_ptr) };

    assert_eq!(
        suffix, 0x2800,
        "huge usable suffix must be raw_alloc_ptr + block_size - user_ptr"
    );
}
// -- Backward-edge free canary tests ------------------------------------------

#[test]
fn free_canary_write_check_clear_roundtrip() {
    use crate::types::Block;
    use crate::constants::MIN_BLOCK_SIZE;

    // Allocate a block-sized buffer with the minimum block size.
    let layout = Layout::from_size_align(MIN_BLOCK_SIZE, MIN_BLOCK_SIZE)
        .expect("valid layout");
    let ptr = unsafe { alloc_zeroed(layout) } as *mut Block;
    assert!(!ptr.is_null());

    let page_cookie: usize = 0xDEAD_BEEF_1234_5678;

    // Initially no canary -- check_double_free should return false.
    let has_canary = unsafe { Block::check_double_free(ptr, page_cookie) };
    assert!(!has_canary, "fresh allocation must not carry a canary");

    // Write the canary.
    unsafe { Block::write_free_canary(ptr, page_cookie) };

    // Now check_double_free should return true.
    let has_canary = unsafe { Block::check_double_free(ptr, page_cookie) };
    assert!(has_canary, "canary must be detected after write_free_canary");

    // A different cookie must NOT match -- the canary is address+cookie bound.
    let wrong_cookie: usize = 0x1111_2222_3333_4444;
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
    use crate::types::Block;
    use crate::constants::MIN_BLOCK_SIZE;

    // Two adjacent blocks with the same page_cookie must produce different canaries.
    let layout = Layout::from_size_align(MIN_BLOCK_SIZE * 2, MIN_BLOCK_SIZE)
        .expect("valid layout");
    let base = unsafe { alloc_zeroed(layout) } as *mut Block;
    assert!(!base.is_null());

    // SAFETY: both pointers are within the MIN_BLOCK_SIZE * 2 allocation.
    let block_a = base;
    let block_b = unsafe { base.add(1) }; // MIN_BLOCK_SIZE offset

    let cookie: usize = 0xCAFE_BABE_0000_0001;

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