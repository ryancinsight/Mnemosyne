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

unsafe impl Send for Segment {}
unsafe impl Sync for Segment {}

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
            let tid = {
                let val: u32;
                // Safety: Inline assembly reading GS register is safe under Windows x86_64.
                unsafe {
                    core::arch::asm!(
                        "mov {0:e}, gs:[0x48]",
                        out(reg) val,
                        options(nostack, preserves_flags, readonly)
                    );
                }
                val
            };
            owner.matches_thread_id(tid)
        }
        #[cfg(any(not(all(windows, target_arch = "x86_64")), miri))]
        {
            owner.matches(get_slot_ptr())
        }
    }
}
