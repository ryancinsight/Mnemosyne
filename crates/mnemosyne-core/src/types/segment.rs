use crate::constants::{PAGE_SIZE, PAGES_PER_SEGMENT};
use crate::types::{Page, SegmentOwner};

/// Metadata representing a segment of memory.
///
/// A segment is a large, aligned virtual memory allocation (typically 2MB).
pub struct Segment {
    /// The original raw allocation pointer returned by the OS.
    ///
    /// Used for tracking and deallocation since OS allocators might require
    /// the original unaligned pointer.
    pub raw_alloc_ptr: *mut u8,
    /// Permission identity for the owner ThreadAllocator cache.
    pub owner: SegmentOwner,
    /// Raw owner allocator cache pointer used after ownership has been proved.
    pub owner_allocator: *mut core::ffi::c_void,
    /// True while this segment is the owner's active page-slicing segment.
    pub is_current: bool,
    /// Pointer to the next segment owned by the same ThreadAllocator.
    pub next_owned_segment: *mut Segment,
    /// Pointer to the previous segment owned by the same ThreadAllocator.
    ///
    /// The owned-segments list is intrusive and doubly linked so a thread can
    /// splice any owned segment out in O(1) during `try_reclaim_segment`
    /// without searching for its predecessor. `Segment` metadata is multiple
    /// kilobytes (it embeds the `[Page; PAGES_PER_SEGMENT]` array), so the
    /// extra back-pointer carries no cache-line cost on the allocation hot
    /// path, which never touches this field.
    pub prev_owned_segment: *mut Segment,
    /// Pointer to the next free segment in the global pool.
    pub next_free_segment: *mut Segment,
    /// If true, free list pointers in this segment are XOR-encrypted.
    pub free_list_encrypted: bool,
    /// NUMA node ID where this segment was allocated.
    pub numa_node: u32,
    /// Mask tracking pages with active allocations.
    ///
    /// The current slicing segment may retain bits for pages that have
    /// returned to zero live allocations. Defragmentation skips the current
    /// segment, and later sweeps validate `alloc_count`, so the mask remains a
    /// conservative reclaim accelerator rather than an ownership authority.
    pub page_occupied_mask: u32,
    /// Mask tracking pages currently linked in the allocator's lists (active, full, empty).
    pub page_linked_mask: u32,
    /// Per-page keys for free-list pointer encryption.
    pub keys: [usize; PAGES_PER_SEGMENT],
    /// The pages metadata array. Page 0 is reserved for segment metadata.
    pub pages: [Page; PAGES_PER_SEGMENT],
}

// SAFETY: `Segment` is a metadata header whose raw pointer fields
// (`raw_alloc_ptr`, `owner_allocator`, the intrusive list links) and interior
// mutability are gated by the segment-ownership protocol: a segment carries an
// opaque `owner` token, and only the thread allocator that can prove token
// equality (`SegmentOwner::matches`/`is_owned_by`) mutates its fields, while
// cross-thread frees route through each page's `AtomicFreeList`. No field is
// thread-affine, so transferring ownership of a `Segment` header between
// threads (`Send`) is sound once the previous owner has released it.
unsafe impl Send for Segment {}
// SAFETY: shared `&Segment` access across threads is sound because the only
// concurrently-mutated state reachable from a shared reference is each page's
// `AtomicFreeList` (itself `Sync`); all non-atomic fields are mutated solely by
// the proven owner under the ownership protocol described above, so a shared
// reference observes no data race.
unsafe impl Sync for Segment {}

/// Recovers the parent segment header and page index for a user pointer.
///
/// Every small allocation lives inside a `SEGMENT_ALIGN`-aligned segment, so
/// masking `ptr` down to `SEGMENT_SIZE` yields the segment header, and the
/// mid-address `PAGE_SHIFT` bits (masked by `PAGES_PER_SEGMENT - 1`) yield the
/// page index. This is the single authoritative pointer→(segment, page_index)
/// classifier shared by the free, realloc, and usable-size fast paths.
///
/// # Safety
///
/// `ptr` must be a non-null pointer returned by a Mnemosyne small/huge
/// allocation, so the recovered segment header is live and the page index is a
/// valid index into its `pages` array.
#[inline(always)]
pub unsafe fn locate_segment(ptr: *mut u8) -> (*mut Segment, usize) {
    let ptr_val = ptr as usize;
    let segment = (ptr_val & !(crate::constants::SEGMENT_SIZE - 1)) as *mut Segment;
    let page_index = (ptr_val >> crate::constants::PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    (segment, page_index)
}

impl Segment {
    /// Initializes a segment header at a given aligned address.
    ///
    /// # Safety
    ///
    /// `aligned_ptr` must be aligned to `SEGMENT_ALIGN` and valid for write.
    pub unsafe fn initialize(aligned_ptr: *mut Segment, raw_alloc_ptr: *mut u8, numa_node: u32) {
        // Safety: aligned_ptr must point to a valid, exclusive, aligned memory segment.
        // We initialize the segment fields and establish parent/child pointers safely.
        unsafe {
            let segment = &mut *aligned_ptr;
            segment.raw_alloc_ptr = raw_alloc_ptr;
            segment.owner = SegmentOwner::NONE;
            segment.owner_allocator = core::ptr::null_mut();
            segment.is_current = false;
            segment.next_owned_segment = core::ptr::null_mut();
            segment.prev_owned_segment = core::ptr::null_mut();
            segment.next_free_segment = core::ptr::null_mut();
            segment.free_list_encrypted = false;
            segment.numa_node = numa_node;
            segment.page_occupied_mask = 0;
            segment.page_linked_mask = 0;
            // Page 0 holds segment metadata and is never allocated from;
            // only pages 1..PAGES_PER_SEGMENT need explicit free-list state.
            // We still initialize page 0 with `Page::new()` so debugging and
            // memory-tracing tools observe uniform metadata across the
            // whole array. No page stores a back-pointer to the segment
            // because every caller recovers it by rounding the page address
            // down to `SEGMENT_ALIGN`.
            for i in 0..PAGES_PER_SEGMENT {
                segment.keys[i] =
                    (aligned_ptr as usize).wrapping_add(i * PAGE_SIZE) ^ 0x5555555555555555;
                segment.pages[i] = Page::new();
                segment.pages[i].page_index = i as u8;
            }
        }
    }

    /// Returns the byte distance from `user_ptr` to the end of the OS-side
    /// mapping for a huge allocation owned by this segment header.
    ///
    /// The mapping starts at `self.raw_alloc_ptr` and has length
    /// `self.pages[0].block_size` (set to `total_alloc_size` by
    /// `allocate_large_or_huge`). Callers that need the usable suffix of a
    /// huge allocation — `usable_size`, the `SecurePolicy` poisoning
    /// sizing, any future bounds-aware huge-alloc accessor — must use
    /// this helper instead of computing `(self as usize) + block_size -
    /// user_ptr`, because the segment header sits at `aligned_addr =
    /// align_up(raw_alloc_ptr, SEGMENT_ALIGN)`, which can be up to
    /// `SEGMENT_ALIGN - 1` bytes past `raw_alloc_ptr`. Using the
    /// segment header as the base would over-report by exactly that
    /// offset and walk callers past the OS mapping boundary.
    ///
    /// # Safety
    ///
    /// `self` must be a segment header initialized by `Segment::initialize`
    /// for a *huge* allocation (`pages[0].block_size > 0`). `user_ptr`
    /// must lie within `[raw_alloc_ptr, raw_alloc_ptr + block_size)`.
    #[inline]
    pub unsafe fn huge_mapping_suffix_from(&self, user_ptr: *const u8) -> usize {
        let huge_size = self.pages[0].block_size;
        debug_assert!(
            huge_size > 0,
            "huge_mapping_suffix_from called on a segment whose pages[0].block_size is zero"
        );
        let raw_ptr_addr = self.raw_alloc_ptr as usize;
        debug_assert!(
            user_ptr as usize >= raw_ptr_addr,
            "user_ptr {:p} precedes raw_alloc_ptr {:p}",
            user_ptr,
            self.raw_alloc_ptr
        );
        debug_assert!(
            user_ptr as usize <= raw_ptr_addr + huge_size,
            "user_ptr {:p} past mapping end (raw_alloc_ptr {:p}, size {})",
            user_ptr,
            self.raw_alloc_ptr,
            huge_size
        );
        (raw_ptr_addr + huge_size) - user_ptr as usize
    }

    /// Returns the free-list encryption cookie for page `page_index` under a
    /// runtime encryption flag: the per-page key when `encrypted`, else `0`.
    ///
    /// This is the single authoritative cookie accessor; the free, realloc,
    /// pop, reclaim, and initialization paths route their `if encrypted {
    /// keys[i] } else { 0 }` selection through it (or the `P`-generic
    /// [`Segment::cookie_for`]) instead of indexing `keys` inline.
    ///
    /// # Safety
    ///
    /// `self` must be this page's parent segment header and `page_index` must be
    /// a valid index into `keys` (`< PAGES_PER_SEGMENT`).
    #[inline(always)]
    pub unsafe fn cookie_for_dynamic(&self, encrypted: bool, page_index: usize) -> usize {
        if encrypted {
            debug_assert!(page_index < PAGES_PER_SEGMENT);
            // SAFETY: the caller's contract guarantees `self` is the valid parent
            // header and `page_index` is in range, so the key read is valid.
            unsafe { *self.keys.get_unchecked(page_index) }
        } else {
            0
        }
    }

    /// Returns the free-list encryption cookie for page `page_index` under the
    /// compile-time policy `P`: the per-page key when `P` encrypts, else `0`.
    ///
    /// The const `P::ENABLE_FREE_LIST_ENCRYPTION` const-propagates into
    /// [`Segment::cookie_for_dynamic`], so the branch resolves at compile time.
    ///
    /// # Safety
    ///
    /// Same contract as [`Segment::cookie_for_dynamic`].
    #[inline(always)]
    pub unsafe fn cookie_for<P: crate::policy::AllocPolicy>(&self, page_index: usize) -> usize {
        // SAFETY: forwarded unchanged from this method's `# Safety` contract.
        unsafe { self.cookie_for_dynamic(P::ENABLE_FREE_LIST_ENCRYPTION, page_index) }
    }

    /// Returns true if this segment is owned by the allocator represented by the given raw slot pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `self` is a valid reference to a `Segment`.
    #[inline(always)]
    pub unsafe fn is_owned_by(
        &self,
        get_slot_ptr: impl FnOnce() -> *mut core::ffi::c_void,
    ) -> bool {
        let owner = self.owner;
        #[cfg(all(windows, target_arch = "x86_64", not(miri)))]
        {
            let _ = get_slot_ptr;
            owner.matches_thread_id(crate::types::current_thread_id())
        }
        #[cfg(any(not(all(windows, target_arch = "x86_64")), miri))]
        {
            owner.matches(get_slot_ptr())
        }
    }
}
