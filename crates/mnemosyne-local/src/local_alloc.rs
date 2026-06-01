//! The thread-local allocator cache managing fast-path operations.
#![allow(clippy::missing_const_for_thread_local)]

use core::marker::PhantomData;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_backend::DefaultBackend;
use mnemosyne_core::constants::{NUM_SIZE_CLASSES, PAGES_PER_SEGMENT};
use mnemosyne_core::types::{Page, Segment};

std::thread_local! {
    static TLS_SEED: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

#[inline(always)]
pub(crate) fn get_tls_seed() -> usize {
    TLS_SEED.with(|cell| {
        let val = cell.get();
        if val == 0 {
            use std::hash::{BuildHasher, Hasher};
            let state = std::collections::hash_map::RandomState::new();
            let mut hasher = state.build_hasher();
            hasher.write_usize(0);
            let mut seed = hasher.finish() as usize;
            if seed == 0 {
                seed = 0xdeadbeeffacefeed;
            }
            cell.set(seed);
            seed
        } else {
            val
        }
    })
}

static CROSS_THREAD_RECLAIMED_BLOCKS: AtomicUsize = AtomicUsize::new(0);

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

#[inline]
pub(crate) fn record_cross_thread_reclaimed(count: usize) {
    CROSS_THREAD_RECLAIMED_BLOCKS.fetch_add(count, Ordering::Relaxed);
}

/// Thread-local cache for fast-path small allocations.
pub struct ThreadAllocator<B: HasSegmentPool = DefaultBackend> {
    /// Active pages per size class.
    pub active_pages: [Option<NonNull<Page>>; NUM_SIZE_CLASSES],
    /// Completely full pages per size class.
    pub full_pages: [Option<NonNull<Page>>; NUM_SIZE_CLASSES],
    /// Stack of empty/defragmented pages available for recycling.
    pub empty_pages: Option<NonNull<Page>>,
    /// Current segment being sliced into pages.
    pub current_segment: Option<NonNull<Segment>>,
    /// Index of the next page to slice in `current_segment`.
    pub next_page_index: usize,
    /// Head of the linked list of segments owned by this thread.
    pub owned_segments_head: *mut Segment,
    /// Number of successful cold-path page refills.
    pub page_refills: usize,
    /// Number of refills served by recycling an initialized empty page.
    pub recycled_pages: usize,
    /// Number of refills served by slicing a never-used page from the current segment.
    pub fresh_pages: usize,
    /// Number of fresh segments acquired by this allocator.
    pub fresh_segments: usize,
    /// Number of orphaned segments adopted by this allocator.
    pub orphan_segments_adopted: usize,
    /// Number of owned-segment sweeps made while searching for recyclable pages.
    pub recycle_sweeps: usize,
    /// Indicates whether an allocation or deallocation operation is currently active on this thread-local cache.
    pub is_allocating: bool,
    /// Thread-local pseudo-random number generator state for allocation randomization.
    pub rng_state: u64,
    /// Marker to bind the generic MemoryBackend parameter.
    pub _phantom: PhantomData<B>,
}

impl<B: HasSegmentPool> Default for ThreadAllocator<B> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<B: HasSegmentPool> ThreadAllocator<B> {
    /// Creates a new, uninitialized `ThreadAllocator`.
    pub const fn new() -> Self {
        Self {
            active_pages: [None; NUM_SIZE_CLASSES],
            full_pages: [None; NUM_SIZE_CLASSES],
            empty_pages: None,
            current_segment: None,
            next_page_index: 0,
            owned_segments_head: core::ptr::null_mut(),
            page_refills: 0,
            recycled_pages: 0,
            fresh_pages: 0,
            fresh_segments: 0,
            orphan_segments_adopted: 0,
            recycle_sweeps: 0,
            is_allocating: false,
            rng_state: 0x123456789abcdefu64,
            _phantom: PhantomData,
        }
    }

    /// Generates the next pseudo-random 64-bit value using Xorshift64,
    /// seeding with the allocator address on the first call to guarantee
    /// different sequences per thread.
    #[inline]
    pub fn next_random(&mut self) -> u64 {
        if self.rng_state == 0x123456789abcdefu64 {
            let addr = self as *const Self as usize as u64;
            self.rng_state ^= addr;
            if self.rng_state == 0 {
                self.rng_state = 0x123456789abcdefu64;
            }
        }
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        x
    }

    /// Returns the process-wide number of blocks reclaimed from cross-thread free lists.
    pub fn cross_thread_reclaimed_blocks() -> usize {
        CROSS_THREAD_RECLAIMED_BLOCKS.load(Ordering::Relaxed)
    }

    /// Returns true when `segment` is the active segment being sliced by this thread.
    #[inline(always)]
    pub fn is_current_segment(&self, segment: *mut Segment) -> bool {
        self.current_segment
            .is_some_and(|current| current.as_ptr() == segment)
    }

    /// Updates the active slicing segment marker.
    ///
    /// # Safety
    ///
    /// Any segment in `segment` and the previous `current_segment` must be
    /// owned exclusively by this allocator while the marker is updated.
    #[inline(always)]
    pub(crate) unsafe fn set_current_segment(&mut self, segment: Option<NonNull<Segment>>) {
        if self.current_segment == segment {
            return;
        }
        if let Some(current) = self.current_segment {
            unsafe {
                (*current.as_ptr()).is_current = false;
            }
        }
        if let Some(next) = segment {
            unsafe {
                (*next.as_ptr()).is_current = true;
            }
        }
        self.current_segment = segment;
    }

    /// Returns a statistics snapshot for this thread allocator.
    pub fn stats(&self) -> ThreadAllocatorStats {
        let mut live_allocations = 0;
        let mut owned_segments = 0;
        let mut size_class_occupancy = [SizeClassOccupancy::default(); NUM_SIZE_CLASSES];
        let mut segment = self.owned_segments_head;
        while !segment.is_null() {
            owned_segments += 1;
            // Safety: segment points to a valid initialized segment owned by this thread.
            // We traverse the pages inside it to collect allocations statistics.
            unsafe {
                for page_index in 1..PAGES_PER_SEGMENT {
                    let page = &(*segment).pages[page_index];
                    live_allocations += page.alloc_count;
                    if page.block_size > 0 {
                        let class = page.size_class as usize;
                        let occupancy = &mut size_class_occupancy[class];
                        occupancy.active_pages += 1;
                        if page.alloc_count == 0 {
                            occupancy.empty_pages += 1;
                        }
                        occupancy.live_allocations += page.alloc_count;
                        occupancy.total_slots += page.max_blocks();
                    }
                }
                segment = (*segment).next_owned_segment;
            }
        }

        ThreadAllocatorStats {
            current_thread_live_allocations: live_allocations,
            current_thread_owned_segments: owned_segments,
            cross_thread_reclaimed_blocks: CROSS_THREAD_RECLAIMED_BLOCKS.load(Ordering::Relaxed),
            page_refills: self.page_refills,
            recycled_pages: self.recycled_pages,
            fresh_pages: self.fresh_pages,
            fresh_segments: self.fresh_segments,
            orphan_segments_adopted: self.orphan_segments_adopted,
            recycle_sweeps: self.recycle_sweeps,
            size_class_occupancy,
        }
    }
}

impl<B: HasSegmentPool> Drop for ThreadAllocator<B> {
    fn drop(&mut self) {
        self.reclaim_owned_segments();
    }
}

unsafe impl<B: HasSegmentPool> Send for ThreadAllocator<B> {}

#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) mod page;
pub(crate) mod routing;
pub(crate) mod segment;
#[cfg(test)]
mod tests;
