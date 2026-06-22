use super::super::*;
use super::fixtures::MockBackend;
use core::ptr::NonNull;
use mnemosyne_arena::{allocate_segment, deallocate_segment};
use mnemosyne_core::constants::{PAGES_PER_SEGMENT, PAGE_SHIFT};
use mnemosyne_core::policy::StandardPolicy;
use mnemosyne_core::size_class::{class_to_size, size_to_class};
use mnemosyne_core::types::Block;

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

#[test]
fn stats_snapshot_counts_active_and_empty_page_lists() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    let mut alloc = ThreadAllocator::<MockBackend>::new();
    let class = size_to_class(16).expect("16 bytes is a small allocation");

    // Safety: `alloc` is initialized and the request is a valid small allocation.
    let ptr = unsafe { alloc.alloc::<StandardPolicy>(16) };
    assert!(!ptr.is_null(), "initial 16-byte allocation failed");

    let live_stats = alloc.stats();
    assert_eq!(live_stats.current_thread_live_allocations, 1);
    assert_eq!(live_stats.current_thread_owned_segments, 1);
    assert_eq!(live_stats.size_class_occupancy[class].active_pages, 1);
    assert_eq!(live_stats.size_class_occupancy[class].empty_pages, 0);
    assert_eq!(live_stats.size_class_occupancy[class].live_allocations, 1);
    assert_eq!(
        live_stats.size_class_occupancy[class].total_slots,
        mnemosyne_core::constants::PAGE_SIZE / mnemosyne_core::constants::MIN_BLOCK_SIZE
    );

    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(mnemosyne_core::constants::SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    let page = unsafe { &mut (*segment).pages[page_index] };

    // Move the page from active to empty using the same intrusive list helpers
    // the allocator uses when a non-current page becomes empty.
    unsafe {
        let block = ptr as *mut Block;
        (*block).set_next::<StandardPolicy>(page.free, 0);
        page.free = Some(NonNull::new_unchecked(block));
        page.set_alloc_count(0);
        alloc.unlink_page(page as *mut Page, class);
        alloc.push_empty_page(NonNull::new_unchecked(page as *mut Page));
    }

    let empty_stats = alloc.stats();
    assert_eq!(empty_stats.current_thread_live_allocations, 0);
    assert_eq!(empty_stats.current_thread_owned_segments, 1);
    // The page moved to the empty recycle list: it is no longer an active page,
    // so active_pages must be zero and total_slots must not include its capacity.
    // The empty_pages counter tracks pages in the recycle pool, not active pages
    // that happen to have zero allocations.
    assert_eq!(empty_stats.size_class_occupancy[class].active_pages, 0);
    assert_eq!(empty_stats.size_class_occupancy[class].empty_pages, 1);
    assert_eq!(empty_stats.size_class_occupancy[class].live_allocations, 0);
    assert_eq!(empty_stats.size_class_occupancy[class].total_slots, 0);
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
