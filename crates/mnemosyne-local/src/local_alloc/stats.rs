use crate::local_alloc::{ThreadAllocator, CROSS_THREAD_RECLAIMED_BLOCKS};
use core::ptr::NonNull;
use core::sync::atomic::Ordering;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use mnemosyne_core::types::Page;

/// Occupancy counters for a single size class in the current thread allocator.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SizeClassOccupancy {
    pub active_pages: usize,
    pub empty_pages: usize,
    pub live_allocations: usize,
    pub total_slots: usize,
}

/// Snapshot of the current thread-local allocator state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadAllocatorStats {
    pub current_thread_live_allocations: usize,
    pub current_thread_owned_segments: usize,
    pub cross_thread_reclaimed_blocks: usize,
    pub page_refills: usize,
    pub recycled_pages: usize,
    pub fresh_pages: usize,
    pub fresh_segments: usize,
    pub orphan_segments_adopted: usize,
    pub recycle_sweeps: usize,
    pub size_class_occupancy: [SizeClassOccupancy; NUM_SIZE_CLASSES],
}

impl Default for ThreadAllocatorStats {
    fn default() -> Self {
        Self {
            current_thread_live_allocations: 0,
            current_thread_owned_segments: 0,
            cross_thread_reclaimed_blocks: 0,
            page_refills: 0,
            recycled_pages: 0,
            fresh_pages: 0,
            fresh_segments: 0,
            orphan_segments_adopted: 0,
            recycle_sweeps: 0,
            size_class_occupancy: [SizeClassOccupancy::default(); NUM_SIZE_CLASSES],
        }
    }
}

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Returns a statistics snapshot for this thread allocator.
    ///
    /// The snapshot walks the allocator's active/full/empty page lists instead
    /// of every page in every owned segment. The page lists are the
    /// authoritative membership structure for initialized pages, so diagnostic
    /// work scales with pages that carry allocator state rather than
    /// `owned_segment_count * PAGES_PER_SEGMENT`.
    pub fn stats(&self) -> ThreadAllocatorStats {
        let mut snapshot = ThreadAllocatorStats {
            current_thread_owned_segments: self.owned_segment_count,
            cross_thread_reclaimed_blocks: CROSS_THREAD_RECLAIMED_BLOCKS.load(Ordering::Relaxed),
            page_refills: self.page_refills,
            recycled_pages: self.recycled_pages,
            fresh_pages: self.fresh_pages,
            fresh_segments: self.fresh_segments,
            orphan_segments_adopted: self.orphan_segments_adopted,
            recycle_sweeps: self.recycle_sweeps,
            ..ThreadAllocatorStats::default()
        };

        for class in 0..NUM_SIZE_CLASSES {
            unsafe { accumulate_active_list(&mut snapshot, self.active_pages[class]) };
            unsafe { accumulate_active_list(&mut snapshot, self.full_pages[class]) };
        }
        // Empty pages are tracked separately: they retain stale size_class/block_size
        // from their last use, so they must not be counted as live active pages.
        unsafe { accumulate_empty_list(&mut snapshot, self.empty_pages) };

        snapshot
    }
}

/// Accumulates stats for pages in an active or full list.
/// Empty pages must not pass through this function — use `accumulate_empty_list`.
unsafe fn accumulate_active_list(
    snapshot: &mut ThreadAllocatorStats,
    mut current: Option<NonNull<Page>>,
) {
    while let Some(page_ptr) = current {
        let page = unsafe { page_ptr.as_ref() };
        if page.block_size > 0 {
            let class = page.size_class as usize;
            debug_assert!(class < NUM_SIZE_CLASSES);
            let occupancy = &mut snapshot.size_class_occupancy[class];
            occupancy.active_pages += 1;
            if page.alloc_count == 0 {
                occupancy.empty_pages += 1;
            }
            occupancy.live_allocations += page.alloc_count;
            occupancy.total_slots += page.max_blocks();
            snapshot.current_thread_live_allocations += page.alloc_count;
        }
        current = page.next_page;
    }
}

/// Accumulates stats for pages in the empty recycle list.
///
/// Empty pages retain stale `size_class`/`block_size` from their last active
/// use, so they must not be counted as live active pages or add to total_slots.
unsafe fn accumulate_empty_list(
    snapshot: &mut ThreadAllocatorStats,
    mut current: Option<NonNull<Page>>,
) {
    while let Some(page_ptr) = current {
        let page = unsafe { page_ptr.as_ref() };
        if page.block_size > 0 {
            let class = page.size_class as usize;
            debug_assert!(class < NUM_SIZE_CLASSES);
            snapshot.size_class_occupancy[class].empty_pages += 1;
        }
        current = page.next_page;
    }
}
