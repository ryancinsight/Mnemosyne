//! Page metadata: a size-classed run of blocks inside a segment, with its
//! local and cross-thread free lists.

use crate::sync::AtomicFreeList;
use crate::types::Block;
use crate::types::Segment;
use core::ptr::NonNull;

/// Metadata representing a page of memory.
///
/// Each page manages blocks of a single size class. The field layout keeps
/// the eight-byte pointer/atomic fields contiguous so the struct stays within
/// a single 64-byte cache line on 64-bit targets, and the back-pointer to the
/// parent segment is omitted because every caller recovers it by rounding the
/// page address down to `SEGMENT_ALIGN`.
pub struct Page {
    /// Thread-local free list of blocks.
    pub free: Option<NonNull<Block>>,
    /// Lock-free list of blocks freed by other threads.
    pub thread_free: AtomicFreeList,
    /// Size of the blocks allocated in this page.
    pub block_size: usize,
    /// Number of active allocations.
    pub alloc_count: usize,
    /// Number of blocks initialized so far (for lazy/bump-allocated fresh pages).
    pub initialized_blocks: usize,
    /// Pointer to the next page in the thread-local size class list.
    pub next_page: Option<NonNull<Page>>,
    /// Pointer to the previous page in the thread-local size class list.
    pub prev_page: Option<NonNull<Page>>,
    /// The size class index of this page.
    pub size_class: u8,
    /// Current list state of this page (0=None, 1=Active, 2=Full, 3=Empty).
    pub list_state: u8,
    /// Index of this page in its parent segment.
    pub page_index: u8,
}

// SAFETY: `Page` is a metadata header embedded in its parent `Segment`. Its
// `NonNull` fields (`free`, `next_page`, `prev_page`) and counters are mutated
// only by the page's proven owner under the segment-ownership protocol; the
// sole field touched by foreign threads is `thread_free`, an `AtomicFreeList`.
// No field is thread-affine, so moving a `Page` header between threads (`Send`)
// is sound once ownership has transferred with its parent segment.
unsafe impl Send for Page {}
// SAFETY: the only state mutated through a shared `&Page` across threads is the
// `thread_free` `AtomicFreeList` (which is itself `Sync`); every other field is
// mutated exclusively by the proven owner, so concurrent shared access observes
// no data race.
unsafe impl Sync for Page {}

impl Page {
    /// Creates a new uninitialized `Page`.
    ///
    /// Non-`const` under `cfg(loom)` only: loom's instrumented atomics cannot be
    /// built in a const context. The shipped allocator keeps the const form.
    #[cfg_attr(not(loom), doc = "")]
    #[cfg(not(loom))]
    pub const fn new() -> Self {
        Self {
            free: None,
            thread_free: AtomicFreeList::new(),
            block_size: 0,
            alloc_count: 0,
            initialized_blocks: 0,
            next_page: None,
            prev_page: None,
            size_class: 0,
            list_state: 0,
            page_index: 0,
        }
    }

    /// Loom-build constructor. See the `cfg(not(loom))` form above.
    #[cfg(loom)]
    pub fn new() -> Self {
        Self {
            free: None,
            thread_free: AtomicFreeList::new(),
            block_size: 0,
            alloc_count: 0,
            initialized_blocks: 0,
            next_page: None,
            prev_page: None,
            size_class: 0,
            list_state: 0,
            page_index: 0,
        }
    }
}

impl Default for Page {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Page {
    /// Returns this page's index within its parent segment's `pages` array.
    ///
    /// `Segment::initialize` assigns this field for every page. Keeping the
    /// index in metadata avoids repeated address-difference division on hot
    /// paths that need a segment key or physical page start.
    #[inline(always)]
    pub fn index_in_segment(&self) -> usize {
        self.page_index as usize
    }

    /// Recovers the parent segment from a raw page pointer.
    ///
    /// The result retains the page pointer's allocation provenance while its
    /// address is aligned down to the segment boundary. Callers must therefore
    /// pass a page pointer projected from the complete segment mapping, not a
    /// pointer reconstructed from an integer address.
    ///
    /// # Safety
    ///
    /// `page` must identify a live page inside its segment mapping.
    #[inline(always)]
    pub unsafe fn parent_segment_of(page: *const Page) -> *mut Segment {
        let segment_addr = page.addr() & !(crate::constants::SEGMENT_SIZE - 1);
        page.map_addr(|_| segment_addr).cast_mut().cast()
    }

    /// Returns an access-capable pointer to a page's physical storage.
    ///
    /// This form derives from the parent segment pointer and therefore
    /// preserves provenance for the complete segment mapping.
    ///
    /// # Safety
    ///
    /// `segment` must identify a live segment allocation and `page_index` must
    /// be in `1..PAGES_PER_SEGMENT`.
    #[inline(always)]
    pub unsafe fn page_start_in_segment(segment: *mut Segment, page_index: usize) -> *mut u8 {
        debug_assert!(page_index > 0 && page_index < crate::constants::PAGES_PER_SEGMENT);
        // SAFETY: the caller guarantees a live full-segment mapping and an
        // in-range page index, so this offset stays inside that allocation.
        unsafe {
            segment
                .cast::<u8>()
                .add(page_index << crate::constants::PAGE_SHIFT)
        }
    }

    /// Returns the maximum number of blocks that can fit in this page.
    #[inline(always)]
    pub fn max_blocks(&self) -> usize {
        crate::size_class::class_to_max_blocks(self.size_class as usize)
    }
}

mod init;
mod occupancy;
mod reclaim;
