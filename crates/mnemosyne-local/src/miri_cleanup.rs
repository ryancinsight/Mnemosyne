//! Orphan-pool teardown used to give miri a quiescent heap at the end
//! of a test run.

#[doc(hidden)]
pub unsafe fn miri_cleanup_pools<B: mnemosyne_arena::HasSegmentPool>() {
    let mut orphaned = std::vec::Vec::new();
    while let Some(segment) = B::global_orphan_pool().pop() {
        orphaned.push(segment);
    }

    for segment in orphaned {
        // SAFETY: `segment` was just popped from the orphan pool, giving
        // exclusive ownership; the header fields are valid and initialized.
        let mut occupied = unsafe { (*segment).page_occupied_mask };
        // SAFETY: same ownership argument as above.
        let encrypted = unsafe { (*segment).free_list_encrypted };
        while occupied != 0 {
            let page_index = occupied.trailing_zeros() as usize;
            occupied &= occupied - 1;
            if page_index != 0 {
                // SAFETY: the caller holds the serialized test lock, and the
                // segment is detached from the orphan pool.
                unsafe {
                    let randomized = (*segment).pages[page_index].secondary_free.is_some();
                    mnemosyne_core::types::Page::reclaim_thread_free_if_present_in_segment_with_randomized(
                        segment,
                        page_index,
                        encrypted,
                        randomized,
                    );
                }
            }
        }

        if unsafe {
            (*segment)
                .pages
                .iter()
                .skip(1)
                .any(|page| page.alloc_count != 0)
        } {
            // SAFETY: `segment` remains live because its allocation count is
            // non-zero, so the detached ownership returns to the orphan pool.
            unsafe { B::global_orphan_pool().push_unbounded(segment) };
        } else {
            // SAFETY: the segment has no live allocations and ownership was
            // transferred from the detached orphan chain to this cleanup.
            unsafe { mnemosyne_arena::deallocate_segment::<B>(segment) };
        }
    }

    // SAFETY: the caller holds the serialized test lock, so no allocator
    // operation can race the detached pools. Live orphan mappings were
    // preserved above so Miri still reports them.
    unsafe { mnemosyne_arena::purge_segment_pool::<B>() };
}
