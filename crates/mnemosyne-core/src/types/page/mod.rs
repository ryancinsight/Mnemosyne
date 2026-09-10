//! Page metadata: a size-classed run of blocks inside a segment, with its
//! local and cross-thread free lists.

use crate::sync::AtomicFreeList;
use crate::types::Block;
use crate::types::Segment;
use core::ptr::NonNull;

#[cfg(target_pointer_width = "64")]
const PARITY_MULTIPLIER: usize = 0x9E37_79B9_7F4A_7C15;

#[cfg(target_pointer_width = "32")]
// The 32-bit word is the low half of the 64-bit mixer constant. Keeping the
// same low-word mixing preserves deterministic parity without an overflowing
// literal on wasm32 and other 32-bit targets.
const PARITY_MULTIPLIER: usize = 0x7F4A_7C15;

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
    /// Alternate free list used by the random-preserve sharding policy.
    ///
    /// This second list keeps the allocator's LIFO reuse pattern from always
    /// returning the same block order on high-contention pages. It is enabled
    /// when a page is initialized with `RANDOMIZE_ALLOCATION` or when a page
    /// hits an active dual-free-list policy. The page still preserves the
    /// single `block_size`/`alloc_count` layout: the second head is just a
    /// compact zero-overhead extension of the page metadata that is checked
    /// with a deterministic parity bit rather than a full `rand` dependency.
    pub secondary_free: Option<NonNull<Block>>,
    /// Lock-free list of blocks freed by other threads.
    pub thread_free: AtomicFreeList,
    /// Size of the blocks allocated in this page.
    pub block_size: u32,
    /// Number of active allocations.
    pub alloc_count: u32,
    /// Number of blocks initialized so far (for lazy/bump-allocated fresh pages).
    pub initialized_blocks: u32,
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
            secondary_free: None,
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
            secondary_free: None,
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

    /// Returns the page-wake threshold for a given size class under a policy's
    /// hysteresis denominator.
    ///
    /// This centralizes the snmalloc-inspired hysteresis rule so every page-list
    /// transition uses the same SSOT for wake eligibility and the tuning knob is
    /// one compile-time constant instead of multiple ad hoc computations.
    #[inline(always)]
    pub fn wake_threshold_for_class(class: usize, denominator: usize) -> usize {
        let max_blocks = crate::size_class::class_to_max_blocks(class);
        max_blocks / denominator.max(1)
    }

    /// Returns `true` when the page has freed enough blocks to re-enter the
    /// active list under the policy's wake-delay hysteresis.
    #[inline(always)]
    pub fn should_reactivate_after_free(
        class: usize,
        alloc_count: usize,
        denominator: usize,
    ) -> bool {
        let max_blocks = crate::size_class::class_to_max_blocks(class);
        let freed_so_far = max_blocks.saturating_sub(alloc_count);
        let wake_threshold = Self::wake_threshold_for_class(class, denominator);
        freed_so_far >= wake_threshold
    }

    /// Returns `true` when the alternate free list should be preferred for this
    /// page, using a deterministic parity bit rather than a runtime RNG.
    ///
    /// This is the low-overhead core of the snmalloc-inspired "random preserve"
    /// policy: the allocator rotates between the primary and secondary lists
    /// without introducing a heap allocation or entropy dependency in the hot
    /// path.
    ///
    /// # Safety
    ///
    /// `page` must identify a live, initialized page whose metadata is owned by
    /// the current allocator context. The function reads `secondary_free` and
    /// `block_size` directly from that page header.
    #[inline(always)]
    pub unsafe fn prefer_secondary_free(page: *mut Page, alloc_count: usize) -> bool {
        // SAFETY: `page` is live initialized page metadata per this function's
        // contract, and the reads below touch only its own header fields.
        if unsafe { (*page).secondary_free }.is_none() {
            return false;
        }
        let block_size = unsafe { (*page).block_size } as usize;
        let seed = page.addr()
            ^ alloc_count.wrapping_mul(PARITY_MULTIPLIER)
            ^ block_size.wrapping_mul(0xD1B54A35);
        (seed & 1) != 0
    }

    /// Returns the policy-specific active free-list head for `page`.
    ///
    /// The decision is centralized here so the allocator keeps one SSOT for the
    /// random-preserve policy instead of reimplementing the same parity check in
    /// the free/reclaim paths.
    ///
    /// # Safety
    /// `page` must point at live, initialized page metadata and stay live for
    /// the call. `alloc_count` is the page's current allocation count, used
    /// only to derive the preference; a wrong value changes which list is
    /// chosen, never whether the chosen head is valid.
    #[inline(always)]
    pub unsafe fn choose_free_head(
        page: *mut Page,
        alloc_count: usize,
        randomized: bool,
    ) -> (Option<NonNull<Block>>, bool) {
        // SAFETY: every read below is of `page`'s own header, which this
        // function's contract guarantees is live and initialized; the
        // `prefer_secondary_free` call forwards that same contract unchanged.
        let has_secondary = unsafe { (*page).secondary_free }.is_some();
        let randomized = randomized || has_secondary;
        let use_secondary = randomized
            && has_secondary
            && (unsafe { (*page).free }.is_none()
                || unsafe { Self::prefer_secondary_free(page, alloc_count) });
        let head = if use_secondary {
            unsafe { (*page).secondary_free }
        } else {
            unsafe { (*page).free }
        };
        (head, use_secondary)
    }
}

mod init;
mod occupancy;
mod reclaim;
