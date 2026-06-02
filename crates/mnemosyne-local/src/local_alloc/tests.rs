use super::page::unlink_page_from_list;
use super::*;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_arena::{allocate_segment, deallocate_segment};
use mnemosyne_core::constants::PAGE_SHIFT;
use mnemosyne_core::policy::StandardPolicy;
use mnemosyne_core::size_class::{class_to_size, size_to_class};
use mnemosyne_core::types::Block;
use mnemosyne_core::MemoryBackend;

// A mock tracking memory backend to verify custom backend injection.
struct MockBackend;
static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static DEALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static MOCK_SEGMENT_POOL: mnemosyne_arena::GlobalSegmentPool =
    mnemosyne_arena::GlobalSegmentPool::new();
static MOCK_ORPHAN_POOL: mnemosyne_arena::GlobalSegmentPool =
    mnemosyne_arena::GlobalSegmentPool::new();
static MOCK_HUGE_POOL: mnemosyne_arena::GlobalHugePool = mnemosyne_arena::GlobalHugePool::new();

impl MemoryBackend for MockBackend {
    unsafe fn allocate(size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        // Safety: delegate to DefaultBackend
        unsafe { DefaultBackend::allocate(size) }
    }

    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        DEALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        // Safety: delegate to DefaultBackend
        unsafe { DefaultBackend::deallocate(ptr, size) }
    }
}

impl mnemosyne_arena::segment::pool::private::Sealed for MockBackend {}

impl HasSegmentPool for MockBackend {
    #[inline(always)]
    fn global_segment_pool() -> &'static mnemosyne_arena::GlobalSegmentPool {
        &MOCK_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static mnemosyne_arena::GlobalSegmentPool {
        &MOCK_ORPHAN_POOL
    }

    #[inline(always)]
    fn global_huge_pool() -> &'static mnemosyne_arena::GlobalHugePool {
        &MOCK_HUGE_POOL
    }
}

crate::impl_local_allocator_selector!(MockBackend);
crate::impl_local_allocator_selector!(DefaultBackend);

#[test]
fn test_custom_backend_injection() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    ALLOC_COUNT.store(0, Ordering::SeqCst);
    DEALLOC_COUNT.store(0, Ordering::SeqCst);

    // Verify that the code compiles with ThreadAllocator parameterized by MockBackend
    let mut alloc = ThreadAllocator::<MockBackend>::new();
    // Safety: alloc is initialized and valid.
    let ptr = unsafe { alloc.alloc::<StandardPolicy>(32) };
    assert!(!ptr.is_null(), "MockBackend small allocation failed");
    unsafe {
        crate::thread_free::<mnemosyne_core::StandardPolicy, MockBackend>(ptr);
    }

    // Verify large allocation directly calls MockBackend
    // Safety: size and align are valid.
    let large_ptr =
        unsafe { mnemosyne_arena::allocate_large_or_huge::<MockBackend>(1024 * 1024, 8, true) };
    assert!(!large_ptr.is_null(), "MockBackend large allocation failed");
    assert!(
        ALLOC_COUNT.load(Ordering::SeqCst) >= 1,
        "MockBackend allocate counter was {}",
        ALLOC_COUNT.load(Ordering::SeqCst)
    );

    // Safety: large_ptr points to huge allocation segment.
    unsafe {
        let seg =
            ((large_ptr as usize) & !(mnemosyne_core::constants::SEGMENT_SIZE - 1)) as *mut Segment;
        let _released = mnemosyne_arena::deallocate_large_or_huge::<MockBackend>(large_ptr, seg);
        mnemosyne_arena::segment::purge_segment_pool::<MockBackend>();
    }
    assert!(
        DEALLOC_COUNT.load(Ordering::SeqCst) >= 1,
        "MockBackend deallocate counter was {}",
        DEALLOC_COUNT.load(Ordering::SeqCst)
    );
}

/// Proves the `#[thread_local]` fast cache still reclaims a terminating
/// thread's owned segments. A `#[thread_local]` static is not dropped on
/// thread exit, so reclamation depends entirely on the exit sentinel
/// (`ThreadExitReclaim`) armed on first allocation. The spawned thread
/// allocates through the TLS path and exits without freeing; the live
/// segment must therefore be orphaned into `MOCK_ORPHAN_POOL`. If the
/// sentinel failed to fire the segment would leak and the pool would stay
/// empty, failing the value-semantic assertion below.
#[cfg(nightly_tls_active)]
#[test]
fn thread_exit_sentinel_reclaims_owned_segments_on_fast_tls_path() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    // Drain any residue so the post-join count reflects only this thread.
    while let Some(seg) = MOCK_ORPHAN_POOL.pop() {
        // Safety: pooled segments are valid mappings owned by the pool.
        unsafe { mnemosyne_arena::deallocate_segment::<MockBackend>(seg) };
    }

    let handle = std::thread::spawn(|| {
        // Safety: a 32-byte/16-align request is a valid small allocation;
        // routing through the TLS path arms the exit sentinel and acquires
        // a segment owned by this thread. The block is intentionally not
        // freed so the owning segment is still live at thread exit.
        let ptr =
            unsafe { crate::thread_alloc::<mnemosyne_core::StandardPolicy, MockBackend>(32, 16) };
        assert!(!ptr.is_null(), "fast-TLS small allocation failed");
        ptr as usize
    });
    let block_addr = handle.join().expect("spawned allocator thread panicked");
    assert_ne!(block_addr, 0, "spawned thread produced a null allocation");

    // The exit sentinel must have orphaned the still-live owning segment.
    let mut reclaimed = 0usize;
    while let Some(seg) = MOCK_ORPHAN_POOL.pop() {
        reclaimed += 1;
        // Safety: pooled segments are valid mappings; release the mapping
        // (including the never-freed block) to avoid leaking the test's
        // owned segment beyond the assertion.
        unsafe { mnemosyne_arena::deallocate_segment::<MockBackend>(seg) };
    }
    assert!(
        reclaimed >= 1,
        "thread-exit sentinel did not reclaim the live owned segment; \
         orphan pool received {reclaimed} segments"
    );
}

#[test]
fn thread_exit_reclaims_owned_segments_on_selected_tls_path() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    // Drain any residue so the post-join count reflects only this thread.
    while let Some(seg) = MOCK_ORPHAN_POOL.pop() {
        // Safety: pooled segments are valid mappings owned by the pool.
        unsafe { mnemosyne_arena::deallocate_segment::<MockBackend>(seg) };
    }

    let handle = std::thread::spawn(|| {
        // Safety: a 32-byte/16-align request is a valid small allocation;
        // routing through the TLS path arms the exit sentinel (or registers standard drop)
        // and acquires a segment owned by this thread. The block is intentionally not
        // freed so the owning segment is still live at thread exit.
        let ptr =
            unsafe { crate::thread_alloc::<mnemosyne_core::StandardPolicy, MockBackend>(32, 16) };
        assert!(!ptr.is_null(), "selected-TLS small allocation failed");
        ptr as usize
    });
    let block_addr = handle.join().expect("spawned allocator thread panicked");
    assert_ne!(block_addr, 0, "spawned thread produced a null allocation");

    // The exit sentinel or slot drop must have orphaned the still-live owning segment.
    let mut reclaimed = 0usize;
    while let Some(seg) = MOCK_ORPHAN_POOL.pop() {
        reclaimed += 1;
        // Safety: pooled segments are valid mappings; release the mapping
        // (including the never-freed block) to avoid leaking the test's
        // owned segment beyond the assertion.
        unsafe { mnemosyne_arena::deallocate_segment::<MockBackend>(seg) };
    }
    assert!(
        reclaimed >= 1,
        "thread-exit did not reclaim the live owned segment; \
         orphan pool received {reclaimed} segments"
    );
}

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
            alloc.push_owned_segment::<StandardPolicy>(ptr);
        }
        seg[i] = ptr;
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
    assert_eq!(alloc.owned_segments_head, seg[0]);
    // Safety: seg0 is the sole remaining live node.
    unsafe { assert!((*seg[0]).prev_owned_segment.is_null()) };

    // Unlink the TAIL/only node (seg0): list becomes empty.
    // Safety: seg0 is the live sole node.
    unsafe { alloc.unlink_owned_segment(seg[0]) };
    assert!(alloc.owned_segments_head.is_null(), "list must be empty");

    // `alloc` now owns no segments; its `Drop` reclamation is a no-op, so
    // the boxed storage is freed safely when `storage` drops here.
}

/// Proves `Page::index_in_segment` (address derivation) equals the stored
/// `page_index` field for every page of a real `SEGMENT_ALIGN`-aligned
/// segment. This pins the correctness of the derivation that lets a future
/// change drop the stored `page_index` field and reclaim its 8 bytes for a
/// doubly-linked `prev_page` back-pointer (O(1) page-list unlink) while
/// keeping `Page` within one 64-byte cache line. A real segment from the
/// backend is used because the derivation rounds the page address down to
/// `SEGMENT_ALIGN`, which only holds for genuinely segment-aligned memory.
#[test]
fn page_address_derivation_index_in_segment() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    // Safety: allocate a real segment-aligned segment from the backend.
    let seg = unsafe { allocate_segment::<DefaultBackend>() }.expect("segment allocation failed");
    assert_eq!(
        seg as usize % mnemosyne_core::constants::SEGMENT_ALIGN,
        0,
        "backend segment is not SEGMENT_ALIGN-aligned"
    );

    for i in 0..PAGES_PER_SEGMENT {
        // Safety: `seg` is a live initialized segment; page `i` is in bounds.
        let page = unsafe { &(*seg).pages[i] };
        assert_eq!(
            page.index_in_segment(),
            i,
            "address derivation disagrees with array position at page {i}"
        );
    }

    // Safety: `seg` was returned by `allocate_segment` and is unaliased.
    unsafe { deallocate_segment::<DefaultBackend>(seg) };
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
    // Safety: all nodes live; `p1` is in the list.
    unsafe {
        unlink_page_from_list(&mut head, n1);
    }
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
    // Safety: `p0` is the head.
    unsafe {
        unlink_page_from_list(&mut head, n0);
    }
    assert_eq!(head, Some(n2));
    unsafe {
        assert_eq!((*p2).prev_page, None);
        assert_eq!((*p2).next_page, None);
        assert_eq!((*p0).next_page, None);
        assert_eq!((*p0).prev_page, None);
    }

    // Unlink the TAIL/only node: list empties.
    // Safety: `p2` is the sole node.
    unsafe {
        unlink_page_from_list(&mut head, n2);
    }
    assert!(head.is_none());

    // Reclaim the raw allocations.
    // Safety: each pointer came from `Box::into_raw` and is unaliased now.
    unsafe {
        drop(std::boxed::Box::from_raw(p0));
        drop(std::boxed::Box::from_raw(p1));
        drop(std::boxed::Box::from_raw(p2));
    }
}

/// Safety regression guard for the guard-free small-allocation fast path.
#[test]
fn unguarded_fast_path_rejects_reentrant_borrow() {
    use crate::LocalAllocatorSelector;
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let outer_saw_reentrant_none = MockBackend::with_allocator(|_outer| {
        // Inside the guarded borrow: is_allocating is set.
        // Safety: the probe closure performs no allocator re-entry.
        let reentrant = unsafe { MockBackend::with_allocator_unguarded(|_inner| 0xC0FFEE_usize) };
        reentrant.is_none()
    });
    assert_eq!(
        outer_saw_reentrant_none,
        Some(true),
        "unguarded fast path aliased a live guarded borrow instead of rejecting re-entry"
    );

    // With no guard held, the unguarded path is permitted and runs `f`.
    // Safety: the closure does not re-enter the allocator.
    let allowed = unsafe { MockBackend::with_allocator_unguarded(|_alloc| 7_usize) };
    assert_eq!(
        allowed,
        Some(7),
        "unguarded path must run the closure when no guard is held"
    );
}

#[test]
fn test_page_recycling_different_classes() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    let mut alloc = ThreadAllocator::<DefaultBackend>::new();

    // 1. Allocate a block of size class 0 (16 bytes now)
    // Safety: alloc is initialized and valid.
    let ptr1 = unsafe { alloc.alloc::<StandardPolicy>(16) };
    assert!(!ptr1.is_null(), "initial 16-byte allocation failed");

    // We should have 1 owned segment now
    let first_stats = alloc.stats();
    assert_eq!(first_stats.current_thread_owned_segments, 1);
    assert_eq!(first_stats.page_refills, 1);

    // Determine which page this block belongs to
    let ptr1_val = ptr1 as usize;
    let segment_addr = ptr1_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr1_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

    // Safety: segment points to a valid segment containing pages.
    let page = unsafe { &mut (*segment).pages[page_index] };

    // 2. Free the block locally by modifying metadata as thread_free would.
    // Since we are not running through thread_free routing, we manually perform a local free.
    // Safety: block ptr is valid and exclusive. We set up page free list.
    unsafe {
        let block = ptr1 as *mut Block;
        (*block).set_next::<StandardPolicy>(page.free, 0);
        page.free = Some(NonNull::new_unchecked(block));
        page.set_alloc_count(0); // Page is now empty
        let class = page.size_class as usize;
        alloc.unlink_page(page as *mut Page, class);
        alloc.push_empty_page(NonNull::new_unchecked(page as *mut Page));
    }

    // 3. Now allocate a block of a DIFFERENT size class, say class 1 (32 bytes).
    // Force current-segment exhaustion so the allocator must sweep owned segments,
    // find the empty page (which was class 0),
    // unlink it from class 0, re-initialize it for class 1, and reuse it.
    alloc.next_page_index = PAGES_PER_SEGMENT;
    // Safety: alloc is initialized and valid.
    let ptr2 = unsafe { alloc.alloc::<StandardPolicy>(32) };
    assert!(!ptr2.is_null(), "recycled 32-byte allocation failed");

    // Assert that allocation stayed within the allocator's bounded owned-segment set.
    assert!(
        alloc.stats().current_thread_owned_segments <= 2,
        "owned segment count exceeded bound: {}",
        alloc.stats().current_thread_owned_segments
    );

    // Verify that allocation reused the owned segment and produced a page for the target class.
    let ptr2_val = ptr2 as usize;
    let segment_addr2 = ptr2_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
    let page_index2 = (ptr2_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    let page2 = unsafe { &(*segment).pages[page_index2] };
    let expected_class = size_to_class(32).expect("32 bytes is a small allocation");

    assert_eq!(segment_addr2, segment_addr);
    assert_eq!(page2.size_class as usize, expected_class);
    assert_eq!(page2.block_size, class_to_size(expected_class));
    assert!(
        page2.alloc_count > 0,
        "recycled page should hold at least one allocation but had {}",
        page2.alloc_count
    );
    unsafe {
        crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(ptr2);
    }

    let recycled_stats = alloc.stats();
    assert!(
        recycled_stats.page_refills >= 2,
        "expected at least 2 page refills after recycle, observed {}",
        recycled_stats.page_refills
    );
    assert!(
        recycled_stats.recycled_pages >= 1,
        "expected at least 1 recycled page after class change, observed {}",
        recycled_stats.recycled_pages
    );
}

#[test]
fn smallest_class_page_saturates_without_duplicate_or_early_refill() {
    // Filling one smallest-class (16-byte) page to capacity must drive
    // `alloc_count` up to `max_blocks` (4096 for a 64 KiB page) with
    // every pointer distinct and non-null.
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    let mut alloc = ThreadAllocator::<DefaultBackend>::new();

    // Allocate the first 16-byte block to materialize the page and learn
    // its capacity.
    // Safety: alloc is initialized and valid.
    let first = unsafe { alloc.alloc::<StandardPolicy>(16) };
    assert!(!first.is_null(), "initial 16-byte allocation failed");

    let first_val = first as usize;
    let segment_addr = first_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (first_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    // Safety: segment points to a valid segment containing pages.
    let max_blocks = unsafe { (*segment).pages[page_index].max_blocks() };
    assert_eq!(
        max_blocks,
        mnemosyne_core::constants::PAGE_SIZE / mnemosyne_core::constants::MIN_BLOCK_SIZE,
        "16-byte page capacity should equal PAGE_SIZE / MIN_BLOCK_SIZE"
    );

    // Drain the remainder of this exact page.
    let mut count = 1usize;
    let mut last = first;
    while count < max_blocks {
        // Safety: alloc is valid.
        let ptr = unsafe { alloc.alloc::<StandardPolicy>(16) };
        assert!(!ptr.is_null(), "16-byte allocation {count} failed");
        let ptr_val = ptr as usize;
        let ptr_seg = ptr_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
        let ptr_page = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
        assert_eq!(
            (ptr_seg, ptr_page),
            (segment_addr, page_index),
            "allocation {count} left the page before saturation: ptr={ptr:p}, expected_segment={segment_addr:#x}, actual_segment={ptr_seg:#x}, expected_page={page_index}, actual_page={ptr_page}, max_blocks={max_blocks}"
        );
        assert_ne!(
            ptr, last,
            "allocator returned a duplicate pointer at {count}"
        );
        last = ptr;
        count += 1;
    }

    // The page's allocation count must now read exactly max_blocks.
    let saturated = unsafe { (*segment).pages[page_index].alloc_count };
    assert_eq!(
        saturated, max_blocks,
        "saturated alloc_count {saturated} != max_blocks {max_blocks}"
    );
    assert!(
        unsafe { (*segment).pages[page_index].free }.is_none(),
        "free list should be empty after saturating the page"
    );

    // One more allocation must succeed by refilling a fresh page rather
    // than returning a wrapped/duplicate pointer.
    // Safety: alloc is valid.
    let overflow = unsafe { alloc.alloc::<StandardPolicy>(16) };
    assert!(!overflow.is_null(), "post-saturation allocation failed");
    let overflow_val = overflow as usize;
    let overflow_seg = overflow_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
    let overflow_page = (overflow_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    assert!(
        overflow_seg != segment_addr || overflow_page != page_index,
        "post-saturation allocation reused the full page"
    );
}

#[test]
fn unlink_full_page_reports_found_status_without_mutating_missing_page() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    let mut alloc = ThreadAllocator::<DefaultBackend>::new();
    let class = size_to_class(16).expect("16 bytes is a small allocation");
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

#[test]
fn test_snmalloc_message_passing() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use std::thread;

    // Purge global segment pool to ensure we must allocate from the OS.
    unsafe {
        mnemosyne_arena::purge_segment_pool::<DefaultBackend>();
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }

    let mut alloc_a = ThreadAllocator::<DefaultBackend>::new();
    // Safety: alloc_a is initialized and valid.
    let ptr = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
    assert!(
        !ptr.is_null(),
        "producer allocation for cross-thread free failed"
    );

    let ptr_usize = ptr as usize;

    // Verify that another thread can free A's block through the owning page queue.
    let handle = thread::spawn(move || {
        // Safety: freeing block allocated by A
        unsafe {
            crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(
                ptr_usize as *mut u8,
            );
        }
    });
    handle.join().expect("cross-thread free worker panicked");

    let mut reclaimed_remote_free = false;
    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    let max_blocks = unsafe { (*segment).pages[page_index].max_blocks() };
    for _ in 0..max_blocks {
        // Safety: alloc_a is valid.
        let ptr2 = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
        assert!(
            !ptr2.is_null(),
            "reclaim probe allocation failed before reclaiming remote free"
        );
        if ptr2 == ptr {
            reclaimed_remote_free = true;
            break;
        }
    }

    assert!(
        reclaimed_remote_free,
        "cross-thread freed block was not reclaimed after {} small allocations",
        max_blocks
    );
}

#[test]
fn test_orphan_segment_reuse() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    use std::sync::mpsc;
    use std::thread;

    unsafe {
        mnemosyne_arena::purge_segment_pool::<DefaultBackend>();
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }

    let (tx, rx) = mpsc::channel();

    // Thread A allocates a block and exits
    thread::spawn(move || {
        let mut alloc_a = ThreadAllocator::<DefaultBackend>::new();
        // Safety: alloc_a is valid.
        let ptr = unsafe { alloc_a.alloc::<StandardPolicy>(32) };
        assert!(!ptr.is_null(), "orphan producer allocation failed");
        tx.send(ptr as usize)
            .expect("orphan producer failed to send live allocation pointer");
    })
    .join()
    .expect("orphan producer thread panicked");

    let live_ptr =
        rx.recv()
            .expect("orphan producer did not send live allocation pointer") as *mut u8;

    // Thread B allocates a block. It should reuse the orphaned segment from A!
    let mut alloc_b = ThreadAllocator::<DefaultBackend>::new();
    // Safety: alloc_b is valid.
    let ptr_b = unsafe { alloc_b.alloc::<StandardPolicy>(64) };
    assert!(!ptr_b.is_null(), "orphan consumer allocation failed");

    // Assert that B reused the orphaned segment: current owned segments must be 1, not 2!
    assert_eq!(alloc_b.stats().current_thread_owned_segments, 1);

    // Free the allocations
    // Safety: pointers are valid and exclusive.
    unsafe {
        crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(live_ptr);
        crate::thread_free::<mnemosyne_core::StandardPolicy, DefaultBackend>(ptr_b);
    }
}

#[test]
fn test_online_defragmentation_page_prioritization() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let mut alloc = ThreadAllocator::<DefaultBackend>::new();

    // Allocate two segments
    let seg1 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg1 allocation failed");
    let seg2 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg2 allocation failed");

    // Make seg1 dirty by setting alloc_count on page 1
    unsafe {
        (*seg1).pages[1].set_alloc_count(1);
        (*seg1).pages[2].set_alloc_count(0);
    }

    // Make seg2 clean by setting alloc_count on all pages to 0
    unsafe {
        for i in 1..mnemosyne_core::constants::PAGES_PER_SEGMENT {
            (*seg2).pages[i].set_alloc_count(0);
        }
    }

    let seg1_page2 = unsafe { NonNull::new_unchecked(&mut (*seg1).pages[2] as *mut Page) };
    let seg2_page1 = unsafe { NonNull::new_unchecked(&mut (*seg2).pages[1] as *mut Page) };

    // Push seg1_page2 first, then seg2_page1 second
    unsafe {
        alloc.push_empty_page(seg1_page2);
        alloc.push_empty_page(seg2_page1);
    }

    // pop_best_empty_page should prioritize the page in seg1 (the dirty segment)
    let popped = unsafe { alloc.pop_best_empty_page() };
    assert_eq!(popped, Some(seg1_page2));

    // The second call should fall back to the clean segment page
    let popped2 = unsafe { alloc.pop_best_empty_page() };
    assert_eq!(popped2, Some(seg2_page1));

    // A third call should return None
    let popped3 = unsafe { alloc.pop_best_empty_page() };
    assert_eq!(popped3, None);

    // Clean up
    unsafe {
        deallocate_segment::<DefaultBackend>(seg1);
        deallocate_segment::<DefaultBackend>(seg2);
    }
}

#[test]
fn test_periodic_defragmentation_segment_reclaim() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    // Case 1: Count < 4. Empty segments should be retained.
    {
        let mut alloc = ThreadAllocator::<DefaultBackend>::new();
        let seg1 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg1 failed");
        let seg2 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg2 failed");
        let seg3 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg3 failed");

        unsafe {
            alloc.push_owned_segment::<StandardPolicy>(seg1);
            alloc.push_owned_segment::<StandardPolicy>(seg2);
            alloc.push_owned_segment::<StandardPolicy>(seg3);
        }

        // Verify we have 3 segments
        let stats = alloc.stats();
        assert_eq!(stats.current_thread_owned_segments, 3);

        // Run sweep
        unsafe {
            alloc.periodic_defragmentation_sweep::<StandardPolicy>();
        }

        // Verify we still have 3 segments (none reclaimed because count < 4)
        let stats = alloc.stats();
        assert_eq!(stats.current_thread_owned_segments, 3);
    }

    // Case 2: Count >= 4. Empty segments should be reclaimed down to 3.
    {
        let mut alloc = ThreadAllocator::<DefaultBackend>::new();
        let seg1 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg1 failed");
        let seg2 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg2 failed");
        let seg3 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg3 failed");
        let seg4 = unsafe { allocate_segment::<DefaultBackend>() }.expect("seg4 failed");

        unsafe {
            alloc.push_owned_segment::<StandardPolicy>(seg1);
            alloc.push_owned_segment::<StandardPolicy>(seg2);
            alloc.push_owned_segment::<StandardPolicy>(seg3);
            alloc.push_owned_segment::<StandardPolicy>(seg4);
        }

        // Set seg1 as the current active segment
        unsafe {
            alloc.set_current_segment(Some(NonNull::new_unchecked(seg1)));
        }

        // Verify we have 4 segments
        let stats = alloc.stats();
        assert_eq!(stats.current_thread_owned_segments, 4);

        // Run sweep
        unsafe {
            alloc.periodic_defragmentation_sweep::<StandardPolicy>();
        }

        // Verify that one segment (seg4, which is head of list, or one of the empty ones)
        // was reclaimed, leaving exactly 3 segments.
        let stats = alloc.stats();
        assert_eq!(stats.current_thread_owned_segments, 3);

        // Verify that seg1 (current active segment) was not reclaimed
        assert!(alloc.is_current_segment(seg1));
    }
}
