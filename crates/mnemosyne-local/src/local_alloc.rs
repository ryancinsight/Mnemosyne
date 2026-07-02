//! The thread-local allocator cache managing fast-path operations.
#![allow(clippy::missing_const_for_thread_local)]

use core::marker::PhantomData;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_backend::DefaultBackend;
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use mnemosyne_core::types::{Page, Segment};

pub use stats::{SizeClassOccupancy, ThreadAllocatorStats};

melinoe::thread_cached! {
    mod tls_seed: usize;
}

#[inline(always)]
pub(crate) fn get_tls_seed() -> usize {
    tls_seed::get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let state = std::collections::hash_map::RandomState::new();
        let mut hasher = state.build_hasher();
        hasher.write_usize(0);
        let mut seed = hasher.finish() as usize;
        if seed == 0 {
            seed = 0xdeadbeeffacefeed;
        }
        seed
    })
}

/// Process-wide fold point for per-thread cross-thread reclamation counts.
///
/// The allocation-side reclaim path (`try_reclaim_and_allocate`, orphan
/// adoption, and the defrag/reclaim sweeps) accumulates into a per-allocator
/// `cross_thread_reclaimed` field instead of touching this atomic directly, so
/// the hot path no longer contends a single global cache line. Each thread
/// folds its accumulated count into this total lazily — on every `stats()`
/// snapshot and once more on `Drop` — so the global reader remains exact.
static CROSS_THREAD_RECLAIMED_BLOCKS: AtomicUsize = AtomicUsize::new(0);

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
    /// Number of nodes linked in `owned_segments_head`.
    pub owned_segment_count: usize,
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
    /// Blocks this allocator has reclaimed from cross-thread (`thread_free`)
    /// lists over its lifetime.
    ///
    /// Accumulated locally on the allocation-side reclaim path to avoid
    /// contending the process-global `CROSS_THREAD_RECLAIMED_BLOCKS` atomic on
    /// every reclaim. A `stats()` snapshot reports the process-wide total as
    /// this field plus the global fold point; `Drop` folds this field into that
    /// global exactly once so a later reader still counts this thread's work.
    pub cross_thread_reclaimed: usize,
    /// Indicates whether an allocation or deallocation operation is currently active on this thread-local cache.
    pub is_allocating: bool,
    /// Thread-local pseudo-random number generator state for allocation randomization.
    pub rng_state: u64,
    /// Counter used to trigger periodic online defragmentation sweeps.
    pub defrag_counter: usize,
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
            owned_segment_count: 0,
            page_refills: 0,
            recycled_pages: 0,
            fresh_pages: 0,
            fresh_segments: 0,
            orphan_segments_adopted: 0,
            recycle_sweeps: 0,
            cross_thread_reclaimed: 0,
            is_allocating: false,
            rng_state: 0x123456789abcdefu64,
            defrag_counter: 0,
            _phantom: PhantomData,
        }
    }

    /// Generates the next pseudo-random 64-bit value using Xorshift64,
    /// seeding with the allocator address on the first call to guarantee
    /// different sequences per thread.
    #[inline]
    pub fn next_random(&mut self) -> u64 {
        if self.rng_state == 0x123456789abcdefu64 {
            let seed = get_tls_seed() as u64;
            let addr = self as *const Self as usize as u64;
            self.rng_state = seed ^ addr;
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

    /// Returns the number of cross-thread-reclaimed blocks folded from threads
    /// that have already terminated.
    ///
    /// This is the process-global fold point read on its own; it does not
    /// include the calling thread's own in-flight `cross_thread_reclaimed`
    /// count (that is added by [`ThreadAllocator::stats`]). It backs the
    /// stats fallback taken only when the current thread has no live allocator,
    /// where the thread contributes nothing of its own.
    pub fn cross_thread_reclaimed_blocks() -> usize {
        CROSS_THREAD_RECLAIMED_BLOCKS.load(Ordering::Relaxed)
    }

    /// Accumulates `count` cross-thread-reclaimed blocks into this allocator's
    /// local counter, avoiding the process-global atomic on the reclaim path.
    #[inline(always)]
    pub(crate) fn record_cross_thread_reclaimed(&mut self, count: usize) {
        self.cross_thread_reclaimed += count;
    }

    /// Returns true when `segment` is the active segment being sliced by this thread.
    #[inline(always)]
    pub fn is_current_segment(&self, segment: *mut Segment) -> bool {
        self.current_segment
            .is_some_and(|current| current.as_ptr() == segment)
    }

    /// Records one cold allocator transition and runs the defragmentation sweep
    /// when the cadence threshold is reached. Hot block alloc/free paths do not
    /// call this helper; sweep cadence is tied to page-level transitions.
    ///
    /// # Safety
    ///
    /// The caller must hold exclusive access to this thread allocator.
    #[inline(always)]
    pub unsafe fn record_defrag_operation<P: mnemosyne_core::AllocPolicy>(&mut self) {
        self.defrag_counter += 1;
        if self.defrag_counter >= 64 {
            // SAFETY: the caller holds exclusive access to this allocator per the
            // `# Safety` contract, which is the precondition the cold sweep needs.
            unsafe { self.run_periodic_defragmentation::<P>() };
        }
    }

    #[cold]
    #[inline(never)]
    unsafe fn run_periodic_defragmentation<P: mnemosyne_core::AllocPolicy>(&mut self) {
        self.defrag_counter = 0;
        if self.is_allocating {
            // SAFETY: `&mut self` is the exclusive borrow of this thread-affine
            // allocator; the sweep walks only this allocator's own page/segment
            // lists. The early return preserves the in-progress `is_allocating`
            // flag so the re-entrant caller restores it.
            unsafe { self.periodic_defragmentation_sweep::<P>() };
            return;
        }

        self.is_allocating = true;
        // SAFETY: as above, `&mut self` grants exclusive access to this
        // allocator's lists; `is_allocating` is raised across the sweep to bar
        // re-entrant fast-path mutation and lowered immediately after.
        unsafe { self.periodic_defragmentation_sweep::<P>() };
        self.is_allocating = false;
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
            // SAFETY: `current` is a segment exclusively owned by this allocator
            // per the `# Safety` contract; clearing `is_current` and pruning
            // empty pages from `page_occupied_mask` touches only its own header.
            // Each `i` comes from the live mask so `pages[i]` is in bounds.
            unsafe {
                let seg_ptr = current.as_ptr();
                (*seg_ptr).is_current = false;
                let mut mask = (*seg_ptr).page_occupied_mask;
                while mask != 0 {
                    let i = mask.trailing_zeros() as usize;
                    mask &= mask - 1;
                    if i > 0 && (*seg_ptr).pages[i].alloc_count == 0 {
                        (*seg_ptr).page_occupied_mask &= !(1 << i);
                    }
                }
            }
        }
        if let Some(next) = segment {
            // SAFETY: `next` is a segment exclusively owned by this allocator per
            // the `# Safety` contract; marking it current writes only its header.
            unsafe {
                (*next.as_ptr()).is_current = true;
            }
        }
        self.current_segment = segment;
    }
}

impl<B: HasSegmentPool> Drop for ThreadAllocator<B> {
    fn drop(&mut self) {
        // Fold this thread's lifetime reclaim count into the process-global
        // total exactly once, before the owned-segment teardown adds any final
        // reclaims. `reclaim_owned_segments` accumulates into
        // `cross_thread_reclaimed`, so drain the field after it runs.
        self.reclaim_owned_segments();
        if self.cross_thread_reclaimed != 0 {
            CROSS_THREAD_RECLAIMED_BLOCKS.fetch_add(self.cross_thread_reclaimed, Ordering::Relaxed);
            self.cross_thread_reclaimed = 0;
        }
    }
}

// SAFETY: `ThreadAllocator` is thread-affine: each instance lives in a
// per-thread `LocalAllocatorSlot` (a `#[thread_local]` static under
// `nightly_tls`, otherwise `std::thread_local!`) and is reached only through
// that owning thread's TLS accessors. The raw `*mut Segment` owned-segment list
// and `NonNull<Page>` arrays it holds are valid only for, and mutated only by,
// that one thread. `Send` is asserted because TLS storage/initialization of the
// slot value requires the contained type to be `Send`-eligible (the instance is
// constructed for, and conceptually moved into, its owning thread's slot); it is
// never shared across threads. The deliberate absence of a `Sync` impl prevents
// `&ThreadAllocator` from crossing threads, so the thread-affine raw pointers are
// never concurrently accessed — `Send`-but-`!Sync` is the exact required bound.
unsafe impl<B: HasSegmentPool> Send for ThreadAllocator<B> {}

#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) mod page;
pub(crate) mod routing;
pub(crate) mod segment;
mod stats;
#[cfg(test)]
mod tests;
