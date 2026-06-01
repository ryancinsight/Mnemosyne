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
    let layout = Layout::from_size_align(
        crate::constants::SEGMENT_SIZE,
        crate::constants::SEGMENT_SIZE,
    )
    .unwrap();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    let page = unsafe { &mut (*segment_ptr).pages[1] };
    page.block_size = 16;

    unsafe {
        let page_start = page.page_start();
        page.initialize_free_list::<crate::policy::StandardPolicy>(page_start, 0);
    }

    let first = unsafe { page.pop_block::<crate::policy::StandardPolicy>() };
    page.alloc_count = 1;
    page.thread_free
        .push::<crate::policy::StandardPolicy>(first);

    let reclaimed = unsafe { page.reclaim_thread_free::<crate::policy::StandardPolicy>() };

    assert_eq!(reclaimed, 1);
    assert_eq!(page.alloc_count, 0);
    assert_eq!(page.free, Some(first));
    assert!(
        page.thread_free.is_empty(),
        "thread_free list was not empty after reclaim"
    );

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}

#[test]
fn test_page_reclaim_thread_free_hot_path() {
    let layout = Layout::from_size_align(
        crate::constants::SEGMENT_SIZE,
        crate::constants::SEGMENT_SIZE,
    )
    .unwrap();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    let page = unsafe { &mut (*segment_ptr).pages[1] };
    page.block_size = 16;

    unsafe {
        let page_start = page.page_start();
        page.initialize_free_list::<crate::policy::StandardPolicy>(page_start, 0);
    }

    let b1 = unsafe { page.pop_block::<crate::policy::StandardPolicy>() };
    let b2 = unsafe { page.pop_block::<crate::policy::StandardPolicy>() };

    // Simulate all other blocks allocated / empty free list
    page.free = None;
    page.alloc_count = 2;

    page.thread_free.push::<crate::policy::StandardPolicy>(b1);
    page.thread_free.push::<crate::policy::StandardPolicy>(b2);

    // Reclaim thread_free. Since page.free is None, this triggers O(1) swap.
    let reclaimed = unsafe { page.reclaim_thread_free::<crate::policy::StandardPolicy>() };

    assert_eq!(reclaimed, 2);
    assert_eq!(page.alloc_count, 0);
    assert_eq!(page.free, Some(b2));

    unsafe {
        let next_node = (*b2.as_ptr()).get_next::<crate::policy::StandardPolicy>(0);
        assert_eq!(next_node, Some(b1));
        assert_eq!(
            (*b1.as_ptr()).get_next::<crate::policy::StandardPolicy>(0),
            None
        );
    }
    assert!(
        page.thread_free.is_empty(),
        "thread_free list was not empty after reclaim"
    );

    unsafe {
        dealloc(segment_ptr as *mut u8, layout);
    }
}

#[test]
fn randomized_page_free_list_uses_seeded_permutation() {
    let layout = Layout::from_size_align(
        crate::constants::SEGMENT_SIZE,
        crate::constants::SEGMENT_SIZE,
    )
    .unwrap();
    let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
    assert!(
        !segment_ptr.is_null(),
        "alloc_zeroed failed to allocate segment"
    );
    let page = unsafe { &mut (*segment_ptr).pages[1] };
    page.block_size = 16;
    page.size_class = 0;

    unsafe {
        let page_start = page.page_start();
        page.initialize_free_list::<RandomizedTestPolicy>(page_start, (7 << 16) | 5);

        let first = page.pop_block::<RandomizedTestPolicy>();
        let second = page.pop_block::<RandomizedTestPolicy>();

        assert_eq!(
            first.as_ptr() as usize - page_start as usize,
            7 * page.block_size,
            "randomized free list must start at the seed-derived index"
        );
        assert_eq!(
            second.as_ptr() as usize - page_start as usize,
            12 * page.block_size,
            "randomized free list must advance by the seed-derived coprime stride"
        );

        dealloc(segment_ptr as *mut u8, layout);
    }
}

#[test]
fn huge_mapping_suffix_uses_raw_mapping_base() {
    let mut segment_storage = core::mem::MaybeUninit::<Segment>::uninit();
    let segment = segment_storage.as_mut_ptr();
    let raw = 0x1000usize as *mut u8;
    unsafe {
        Segment::initialize(segment, raw, 0);
        (*segment).pages[0].block_size = 0x4000;
    }

    let user_ptr = 0x2800usize as *const u8;
    let suffix = unsafe { (*segment).huge_mapping_suffix_from(user_ptr) };

    assert_eq!(
        suffix, 0x2800,
        "huge usable suffix must be raw_alloc_ptr + block_size - user_ptr"
    );
}
