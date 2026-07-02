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

    /// Recovers a raw pointer to this page's parent segment header.
    ///
    /// Every `Page` lives inside its parent segment's `pages` array, and the
    /// segment header is `SEGMENT_ALIGN`-aligned at the base of the mapping, so
    /// masking the page's own address down to `SEGMENT_SIZE` yields the header.
    /// This is the single authoritative segment-recovery accessor; the free,
    /// realloc, reclaim, and occupancy paths route through it instead of
    /// repeating the `addr & !(SEGMENT_SIZE - 1)` mask (and its SAFETY argument)
    /// inline.
    ///
    /// The returned pointer is only valid while this page is part of a live,
    /// mapped segment; callers dereference it under their own segment-ownership
    /// invariant.
    #[inline(always)]
    pub fn parent_segment(&self) -> *mut Segment {
        let self_addr = self as *const Page as usize;
        (self_addr & !(crate::constants::SEGMENT_SIZE - 1)) as *mut Segment
    }

    /// Returns the physical start address of this page in memory.
    #[inline(always)]
    pub fn page_start(&self) -> *mut u8 {
        let self_addr = self as *const Page as usize;
        let segment_addr = self_addr & !(crate::constants::SEGMENT_SIZE - 1);
        let page_offset = (self.page_index as usize) << crate::constants::PAGE_SHIFT;
        // SAFETY: `segment_addr` is the base of `self`'s parent segment mapping
        // (the address of `self` masked down to `SEGMENT_SIZE`), and
        // `page_offset = page_index * PAGE_SIZE` with `page_index <
        // PAGES_PER_SEGMENT`, so the result stays within the single
        // `SEGMENT_SIZE` allocation that contains this page — the offset is in
        // bounds of that object.
        unsafe { (segment_addr as *mut u8).add(page_offset) }
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
