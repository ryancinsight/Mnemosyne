use super::fixtures::MockBackend;
use super::super::*;
use super::super::page::{unlink_page_from_list, with_page_list_token};
use core::ptr::NonNull;
use mnemosyne_core::policy::StandardPolicy;
use mnemosyne_core::types::Page;

/// Verifies the intrusive doubly-linked owned-segments list splices any
/// node out in place and in O(1) — without searching for a predecessor —
/// while preserving the `prev`/`next` invariant across head, middle, and
/// tail removals. This pins the correctness of `push_owned_segment` and the
/// O(1) `unlink_owned_segment` that replaced the prior linear search.
#[test]
fn owned_segment_list_is_doubly_linked_and_unlinks_in_place() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    let mut alloc = ThreadAllocator::<DefaultBackend>::new();

    // Three standalone segment headers. Alignment is irrelevant here: the
    // list helpers only touch the `prev`/`next` link fields, never the page
    // data, so heap-boxed storage is sufficient and avoids real OS mappings.
    let mut storage: [std::boxed::Box<core::mem::MaybeUninit<Segment>>; 3] = [
        std::boxed::Box::new(core::mem::MaybeUninit::uninit()),
        std::boxed::Box::new(core::mem::MaybeUninit::uninit()),
        std::boxed::Box::new(core::mem::MaybeUninit::uninit()),
    ];
    let mut seg = [core::ptr::null_mut::<Segment>(); 3];
    for (i, slot) in storage.iter_mut().enumerate() {
        let ptr = slot.as_mut_ptr();
        // Safety: `ptr` is a unique, writable, suitably sized allocation.
        unsafe {
            Segment::initialize(ptr, core::ptr::null_mut(), 0);
            assert!(
                (*ptr).owner_allocator.is_null(),
                "initialized segment owner_allocator must start null"
            );
            alloc.push_owned_segment::<StandardPolicy>(ptr);
        }
        seg[i] = ptr;
        assert_eq!(
            unsafe { (*ptr).owner_allocator },
            (&mut alloc as *mut ThreadAllocator<DefaultBackend>).cast(),
            "owned segment must cache the owning allocator pointer"
        );
        assert_eq!(
            alloc.owned_segment_count,
            i + 1,
            "owned_segment_count must track each owned-list insertion"
        );
    }

    // Pushing seg0, seg1, seg2 yields head: seg2 -> seg1 -> seg0.
    assert_eq!(
        alloc.owned_segments_head, seg[2],
        "head must be last pushed"
    );
    // Safety: all three pointers are live boxed segments linked above.
    unsafe {
        assert!((*seg[2]).prev_owned_segment.is_null());
        assert_eq!((*seg[2]).next_owned_segment, seg[1]);
        assert_eq!((*seg[1]).prev_owned_segment, seg[2]);
        assert_eq!((*seg[1]).next_owned_segment, seg[0]);
        assert_eq!((*seg[0]).prev_owned_segment, seg[1]);
        assert!((*seg[0]).next_owned_segment.is_null());
    }

    // Unlink the MIDDLE node (seg1): list becomes seg2 -> seg0 with no
    // predecessor search.
    // Safety: seg1 is a live node in this list.
    unsafe { alloc.unlink_owned_segment(seg[1]) };
    assert_eq!(
        alloc.owned_segment_count, 2,
        "unlinking the middle segment must decrement owned_segment_count"
    );
    assert_eq!(alloc.owned_segments_head, seg[2]);
    // Safety: pointers remain live (storage outlives this scope).
    unsafe {
        assert_eq!((*seg[2]).next_owned_segment, seg[0]);
        assert_eq!((*seg[0]).prev_owned_segment, seg[2]);
        // The detached node carries no stale list pointers.
        assert!((*seg[1]).prev_owned_segment.is_null());
        assert!((*seg[1]).next_owned_segment.is_null());
    }

    // Unlink the HEAD node (seg2): list becomes seg0.
    // Safety: seg2 is the live head node.
    unsafe { alloc.unlink_owned_segment(seg[2]) };
    assert_eq!(
        alloc.owned_segment_count, 1,
        "unlinking the head segment must decrement owned_segment_count"
    );
    assert_eq!(alloc.owned_segments_head, seg[0]);
    // Safety: seg0 is the sole remaining live node.
    unsafe { assert!((*seg[0]).prev_owned_segment.is_null()) };

    // Unlink the TAIL/only node (seg0): list becomes empty.
    // Safety: seg0 is the live sole node.
    unsafe { alloc.unlink_owned_segment(seg[0]) };
    assert_eq!(
        alloc.owned_segment_count, 0,
        "unlinking the final segment must clear owned_segment_count"
    );
    assert!(alloc.owned_segments_head.is_null(), "list must be empty");

    // `alloc` now owns no segments; its `Drop` reclamation is a no-op, so
    // the boxed storage is freed safely when `storage` drops here.
}

/// Validates the singly-linked page-list splice helper `unlink_page_from_list`
/// across head, middle, tail, and absent-target cases. Page nodes are held
/// as fully raw allocations (`Box::into_raw`) so the test never interleaves
/// `Box` and raw-pointer access — keeping it clean under Miri's Stacked
/// Borrows checker, which has no FFI-backed allocator path to exercise this
/// logic otherwise.
#[test]
fn unlink_page_from_list_splices_and_reports_membership() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    // Three standalone page nodes owned as raw pointers for the duration.
    let p0 = std::boxed::Box::into_raw(std::boxed::Box::new(Page::new()));
    let p1 = std::boxed::Box::into_raw(std::boxed::Box::new(Page::new()));
    let p2 = std::boxed::Box::into_raw(std::boxed::Box::new(Page::new()));
    // Safety: p0/p1/p2 are unique live allocations; build head -> p0 -> p1 -> p2.
    let (n0, n1, n2) = unsafe {
        let n0 = NonNull::new_unchecked(p0);
        let n1 = NonNull::new_unchecked(p1);
        let n2 = NonNull::new_unchecked(p2);
        (*p0).next_page = Some(n1);
        (*p0).prev_page = None;
        (*p1).next_page = Some(n2);
        (*p1).prev_page = Some(n0);
        (*p2).next_page = None;
        (*p2).prev_page = Some(n1);
        (n0, n1, n2)
    };
    let mut head = Some(n0);

    // Unlink the MIDDLE node: head -> p0 -> p2, p1 detached.
    // Safety: all nodes live and are treated as belonging to this test's
    // branded page-list permission.
    with_page_list_token::<MockBackend, _>(|mut token| unsafe {
        let page = token.page(n1);
        unlink_page_from_list(&mut token, &mut head, page);
    });
    assert_eq!(head, Some(n0));
    // Safety: nodes remain live.
    unsafe {
        assert_eq!((*p0).next_page, Some(n2));
        assert_eq!((*p0).prev_page, None);
        assert_eq!((*p2).prev_page, Some(n0));
        assert_eq!((*p2).next_page, None);
        assert_eq!((*p1).next_page, None);
        assert_eq!((*p1).prev_page, None);
    }

    // Unlink the HEAD node: head -> p2.
    // Safety: `p0` is the head and belongs to the same branded test list.
    with_page_list_token::<MockBackend, _>(|mut token| unsafe {
        let page = token.page(n0);
        unlink_page_from_list(&mut token, &mut head, page);
    });
    assert_eq!(head, Some(n2));
    unsafe {
        assert_eq!((*p2).prev_page, None);
        assert_eq!((*p2).next_page, None);
        assert_eq!((*p0).next_page, None);
        assert_eq!((*p0).prev_page, None);
    }

    // Unlink the TAIL/only node: list empties.
    // Safety: `p2` is the sole node and belongs to the same branded test list.
    with_page_list_token::<MockBackend, _>(|mut token| unsafe {
        let page = token.page(n2);
        unlink_page_from_list(&mut token, &mut head, page);
    });
    assert!(head.is_none());

    // Reclaim the raw allocations.
    // Safety: each pointer came from `Box::into_raw` and is unaliased now.
    unsafe {
        drop(std::boxed::Box::from_raw(p0));
        drop(std::boxed::Box::from_raw(p1));
        drop(std::boxed::Box::from_raw(p2));
    }
}

#[test]
fn unlink_full_page_reports_found_status_without_mutating_missing_page() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let class = mnemosyne_core::size_class::size_to_class(16).expect("16 bytes is a small allocation");
    let mut listed = Page::new();
    let mut missing = Page::new();
    listed.list_state = 2; // Simulate being in full_pages
    let listed_ptr = NonNull::from(&mut listed);
    let missing_ptr = NonNull::from(&mut missing);
    alloc.full_pages[class] = Some(listed_ptr);

    let removed_missing = unsafe { alloc.unlink_full_page(missing_ptr.as_ptr(), class) };

    assert!(
        !removed_missing,
        "unlink_full_page reported removal for a page outside the full list"
    );
    assert_eq!(
        alloc.full_pages[class].map(NonNull::as_ptr),
        Some(listed_ptr.as_ptr())
    );
    assert_eq!(missing.next_page, None);

    let removed_listed = unsafe { alloc.unlink_full_page(listed_ptr.as_ptr(), class) };

    assert!(
        removed_listed,
        "unlink_full_page did not report removal for the listed page"
    );
    assert_eq!(alloc.full_pages[class], None);
    assert_eq!(listed.next_page, None);
}
