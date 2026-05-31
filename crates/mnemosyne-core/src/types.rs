//! Core memory layout types: Block, Page, and Segment.

use crate::constants::{PAGE_SIZE, PAGES_PER_SEGMENT};
use crate::sync::AtomicFreeList;
use core::ptr::NonNull;

/// A node representing a free block.
///
/// Free blocks are stored inline within the allocated memory when free.
#[repr(transparent)]
pub struct Block {
    /// Encrypted or raw pointer to the next free block.
    next_encoded: Option<NonNull<Block>>,
}

impl Block {
    /// Gets the next block in the free list, decoding it if required.
    ///
    /// # Safety
    ///
    /// The block pointer must be valid and aligned.
    #[inline(always)]
    pub unsafe fn get_next<P: crate::policy::AllocPolicy>(
        &self,
        page_cookie: usize,
    ) -> Option<NonNull<Block>> {
        if P::ENABLE_FREE_LIST_ENCRYPTION {
            self.next_encoded.map(|encoded| {
                let cookie = page_cookie | 1;
                let decoded_ptr = (encoded.as_ptr() as usize ^ cookie) as *mut Block;
                unsafe { NonNull::new_unchecked(decoded_ptr) }
            })
        } else {
            self.next_encoded
        }
    }

    /// Gets the next block dynamically using a dynamic encrypted flag.
    ///
    /// # Safety
    ///
    /// The block pointer must be valid and aligned.
    #[inline(always)]
    pub unsafe fn get_next_dynamic(
        &self,
        encrypted: bool,
        page_cookie: usize,
    ) -> Option<NonNull<Block>> {
        if encrypted {
            self.next_encoded.map(|encoded| {
                let cookie = page_cookie | 1;
                let decoded_ptr = (encoded.as_ptr() as usize ^ cookie) as *mut Block;
                unsafe { NonNull::new_unchecked(decoded_ptr) }
            })
        } else {
            self.next_encoded
        }
    }

    /// Sets the next block in the free list, encoding it if required.
    ///
    /// # Safety
    ///
    /// The block pointer must be valid and aligned.
    #[inline(always)]
    pub unsafe fn set_next<P: crate::policy::AllocPolicy>(
        &mut self,
        next: Option<NonNull<Block>>,
        page_cookie: usize,
    ) {
        if P::ENABLE_FREE_LIST_ENCRYPTION {
            self.next_encoded = next.map(|ptr| {
                let cookie = page_cookie | 1;
                let encoded_ptr = (ptr.as_ptr() as usize ^ cookie) as *mut Block;
                unsafe { NonNull::new_unchecked(encoded_ptr) }
            });
        } else {
            self.next_encoded = next;
        }
    }

    /// Sets the next block dynamically using a dynamic encrypted flag.
    ///
    /// # Safety
    ///
    /// The block pointer must be valid and aligned.
    #[inline(always)]
    pub unsafe fn set_next_dynamic(
        &mut self,
        next: Option<NonNull<Block>>,
        encrypted: bool,
        page_cookie: usize,
    ) {
        if encrypted {
            self.next_encoded = next.map(|ptr| {
                let cookie = page_cookie | 1;
                let encoded_ptr = (ptr.as_ptr() as usize ^ cookie) as *mut Block;
                unsafe { NonNull::new_unchecked(encoded_ptr) }
            });
        } else {
            self.next_encoded = next;
        }
    }
}

// Block is simple data, safe to send/sync as a memory representation.
unsafe impl Send for Block {}
unsafe impl Sync for Block {}

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
    pub size_class: u32,
    /// Current list state of this page (0=None, 1=Active, 2=Full, 3=Empty).
    pub list_state: u32,
}

unsafe impl Send for Page {}
unsafe impl Sync for Page {}

/// Permission identity for the thread allocator that owns a segment.
///
/// This follows the GhostCell separation principle at allocator scale: segment
/// data stores an opaque ownership token, while mutation permission remains with
/// the thread-local allocator that can prove token equality. The representation
/// is a raw pointer-sized value, so checks compile to the same pointer
/// comparison as the previous untyped field.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentOwner(pub usize);

impl SegmentOwner {
    /// No thread currently owns this segment.
    pub const NONE: Self = Self(0);

    /// Builds an owner token from an allocator pointer.
    #[inline(always)]
    pub fn from_ptr<T>(ptr: *mut T) -> Self {
        Self(ptr as usize)
    }

    /// Returns true when this token identifies `ptr`.
    #[inline(always)]
    pub fn matches<T>(self, ptr: *mut T) -> bool {
        self.0 == ptr as usize
    }

    /// Builds an owner token from a thread ID.
    #[inline(always)]
    pub fn from_thread_id(tid: u32) -> Self {
        Self(tid as usize)
    }

    /// Returns true when this token identifies `tid`.
    #[inline(always)]
    pub fn matches_thread_id(self, tid: u32) -> bool {
        self.0 == tid as usize
    }
}

unsafe impl Send for SegmentOwner {}
unsafe impl Sync for SegmentOwner {}

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
        }
    }

    /// Recovers this page's index within its parent segment's `pages` array
    /// from the page's own metadata address, in O(1).
    ///
    /// The page metadata lives at
    /// `segment_base + offset_of!(Segment, pages) + index * size_of::<Page>()`,
    /// so the index is `(self_addr - pages_base) / size_of::<Page>()`. The
    /// segment base is recovered by rounding the page address down to
    /// `SEGMENT_ALIGN` (`== SEGMENT_SIZE`), the same masking the small-free
    /// classifier already relies on. `Page` is exactly 64 bytes (a power of
    /// two), so the division lowers to a shift.
    ///
    /// This derivation makes the stored `page_index` field redundant: removing
    /// it frees an 8-byte slot for a doubly-linked `prev_page` back-pointer
    /// (enabling O(1) page-list unlink) while keeping `Page` within its single
    /// 64-byte cache line. `page_index_field_matches_address_derivation` pins
    /// the equivalence across every page of a real initialized segment.
    ///
    /// # Safety
    ///
    /// `self` must be a `Page` embedded in the `pages` array of a
    /// `SEGMENT_ALIGN`-aligned `Segment` (the only context pages are created in).
    #[inline]
    pub fn index_in_segment(&self) -> usize {
        let self_addr = self as *const Page as usize;
        let segment_addr = self_addr & !(crate::constants::SEGMENT_SIZE - 1);
        let pages_base = segment_addr + core::mem::offset_of!(Segment, pages);
        (self_addr - pages_base) / core::mem::size_of::<Page>()
    }

    /// Returns the physical start address of this page in memory.
    #[inline(always)]
    pub fn page_start(&self) -> *mut u8 {
        let self_addr = self as *const Page as usize;
        let segment_addr = self_addr & !(crate::constants::SEGMENT_SIZE - 1);
        let offset = self_addr - segment_addr - core::mem::offset_of!(Segment, pages);
        // Since Page is 64 bytes (2^6) and PAGE_SIZE is 65536 bytes (2^16),
        // the page start offset is page_index * PAGE_SIZE = (offset / 64) * 65536
        // = (offset >> 6) << 16 = offset << 10.
        // The low 6 bits of offset are 0 because Page is 64-byte aligned,
        // so shift left by 10 is perfectly precise and avoids an intermediate right-shift.
        let page_offset = offset << (crate::constants::PAGE_SHIFT - core::mem::size_of::<Page>().trailing_zeros() as usize);
        unsafe { (segment_addr as *mut u8).add(page_offset) }
    }

    /// Returns the maximum number of blocks that can fit in this page.
    #[inline(always)]
    pub fn max_blocks(&self) -> usize {
        crate::size_class::class_to_max_blocks(self.size_class as usize)
    }

    /// Pops a block from the page's local free list, using lazy/bump allocation if necessary.
    ///
    /// # Safety
    ///
    /// The page must have free blocks or uninitialized blocks remaining.
    #[inline(always)]
    pub unsafe fn pop_block<P: crate::policy::AllocPolicy>(&mut self) -> NonNull<Block> {
        if let Some(block) = self.free {
            let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
                let self_addr = self as *const Page as usize;
                let segment_addr = self_addr & !(crate::constants::SEGMENT_SIZE - 1);
                let segment = segment_addr as *mut Segment;
                let page_index = self.index_in_segment();
                unsafe { (*segment).keys[page_index] }
            } else {
                0
            };
            self.free = unsafe { (*block.as_ptr()).get_next::<P>(cookie) };
            block
        } else if self.initialized_blocks < self.max_blocks() {
            let idx = self.initialized_blocks;
            self.initialized_blocks += 1;
            let page_start = self.page_start();
            let block_ptr = unsafe { page_start.add(idx * self.block_size) } as *mut Block;
            unsafe { NonNull::new_unchecked(block_ptr) }
        } else {
            unsafe { core::hint::unreachable_unchecked() }
        }
    }

    /// Atomically drains cross-thread frees into the page-local free list dynamically.
    ///
    /// # Safety
    ///
    /// The page must belong to the allocator context currently reconciling its
    /// metadata.
    #[inline]
    pub unsafe fn reclaim_thread_free_dynamic(&mut self, encrypted: bool) -> usize {
        let cookie = if encrypted {
            let self_addr = self as *const Page as usize;
            let segment_addr = self_addr & !(crate::constants::SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;
            let page_index = self.index_in_segment();
            unsafe { (*segment).keys[page_index] }
        } else {
            0
        };

        let Some((block, count)) = self.thread_free.pop_all(encrypted, cookie) else {
            return 0;
        };

        debug_assert!(
            self.alloc_count >= count,
            "reclaim count {} exceeds page allocation count {}",
            count,
            self.alloc_count
        );
        self.alloc_count -= count;

        if self.free.is_none() {
            self.free = Some(block);
        } else {
            let mut last = block;
            while let Some(node) = unsafe { (*last.as_ptr()).get_next_dynamic(encrypted, cookie) } {
                last = node;
            }
            unsafe {
                (*last.as_ptr()).set_next_dynamic(self.free, encrypted, cookie);
            }
            self.free = Some(block);
        }
        count
    }

    /// Atomically drains cross-thread frees into the page-local free list.
    ///
    /// # Safety
    ///
    /// The page must belong to the allocator context currently reconciling its
    /// metadata.
    #[inline]
    pub unsafe fn reclaim_thread_free<P: crate::policy::AllocPolicy>(&mut self) -> usize {
        unsafe { self.reclaim_thread_free_dynamic(P::ENABLE_FREE_LIST_ENCRYPTION) }
    }

    /// Initializes a page's free list for a specific block size.
    ///
    /// # Safety
    ///
    /// The `page_start` pointer must point to the start of the 64KB page
    /// and must be valid for reads and writes of size `PAGE_SIZE`.
    pub unsafe fn initialize_free_list<P: crate::policy::AllocPolicy>(
        &mut self,
        _page_start: *mut u8,
    ) {
        self.initialized_blocks = 0;
        self.alloc_count = 0;
        self.free = None;
    }
}

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
            segment.is_current = false;
            segment.next_owned_segment = core::ptr::null_mut();
            segment.prev_owned_segment = core::ptr::null_mut();
            segment.next_free_segment = core::ptr::null_mut();
            segment.free_list_encrypted = false;
            segment.numa_node = numa_node;
            for i in 0..PAGES_PER_SEGMENT {
                segment.keys[i] =
                    (aligned_ptr as usize).wrapping_add(i * PAGE_SIZE) ^ 0x5555555555555555;
            }

            // Page 0 holds segment metadata and is never allocated from;
            // only pages 1..PAGES_PER_SEGMENT need explicit free-list state.
            // We still initialize page 0 with `Page::new()` so debugging and
            // memory-tracing tools observe uniform metadata across the
            // whole array. No page stores a back-pointer to the segment
            // because every caller recovers it by rounding the page address
            // down to `SEGMENT_ALIGN`.
            for i in 0..PAGES_PER_SEGMENT {
                segment.pages[i] = Page::new();
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
}

#[cfg(test)]
mod tests {
    use ::std::alloc::{alloc_zeroed, dealloc, Layout};
    use super::*;

    #[test]
    fn page_struct_size_stays_within_one_cache_line() {
        // Page metadata is hot: every allocation reads and writes
        // `page.free`, `page.alloc_count`, and `page.block_size`. Keeping
        // the struct within a single 64-byte cache line on 64-bit targets
        // ensures the fast path touches only one cache line per page
        // operation.
        assert!(
            core::mem::size_of::<Page>() <= 64,
            "Page exceeds one 64-byte cache line ({} bytes)",
            core::mem::size_of::<Page>()
        );
    }

    #[test]
    fn test_page_reclaim_thread_free() {
        let layout = Layout::from_size_align(
            crate::constants::SEGMENT_SIZE,
            crate::constants::SEGMENT_SIZE,
        )
        .unwrap();
        let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
        assert!(!segment_ptr.is_null(), "alloc_zeroed failed to allocate segment");
        let page = unsafe { &mut (*segment_ptr).pages[1] };
        page.block_size = 16;

        unsafe {
            let page_start = page.page_start();
            page.initialize_free_list::<crate::policy::StandardPolicy>(page_start);
        }

        let first = unsafe { page.pop_block::<crate::policy::StandardPolicy>() };
        page.alloc_count = 1;
        page.thread_free
            .push::<crate::policy::StandardPolicy>(first);

        let reclaimed = unsafe { page.reclaim_thread_free::<crate::policy::StandardPolicy>() };

        assert_eq!(reclaimed, 1);
        assert_eq!(page.alloc_count, 0);
        assert_eq!(page.free, Some(first));
        assert!(
            page.thread_free.is_empty(),
            "thread_free list was not empty after reclaim"
        );

        unsafe {
            dealloc(segment_ptr as *mut u8, layout);
        }
    }

    #[test]
    fn test_page_reclaim_thread_free_hot_path() {
        let layout = Layout::from_size_align(
            crate::constants::SEGMENT_SIZE,
            crate::constants::SEGMENT_SIZE,
        )
        .unwrap();
        let segment_ptr = unsafe { alloc_zeroed(layout) as *mut Segment };
        assert!(!segment_ptr.is_null(), "alloc_zeroed failed to allocate segment");
        let page = unsafe { &mut (*segment_ptr).pages[1] };
        page.block_size = 16;

        unsafe {
            let page_start = page.page_start();
            page.initialize_free_list::<crate::policy::StandardPolicy>(page_start);
        }

        let b1 = unsafe { page.pop_block::<crate::policy::StandardPolicy>() };
        let b2 = unsafe { page.pop_block::<crate::policy::StandardPolicy>() };

        // Simulate all other blocks allocated / empty free list
        page.free = None;
        page.alloc_count = 2;

        page.thread_free.push::<crate::policy::StandardPolicy>(b1);
        page.thread_free.push::<crate::policy::StandardPolicy>(b2);

        // Reclaim thread_free. Since page.free is None, this triggers O(1) swap.
        let reclaimed = unsafe { page.reclaim_thread_free::<crate::policy::StandardPolicy>() };

        assert_eq!(reclaimed, 2);
        assert_eq!(page.alloc_count, 0);
        assert_eq!(page.free, Some(b2));

        unsafe {
            let next_node = (*b2.as_ptr()).get_next::<crate::policy::StandardPolicy>(0);
            assert_eq!(next_node, Some(b1));
            assert_eq!(
                (*b1.as_ptr()).get_next::<crate::policy::StandardPolicy>(0),
                None
            );
        }
        assert!(
            page.thread_free.is_empty(),
            "thread_free list was not empty after reclaim"
        );

        unsafe {
            dealloc(segment_ptr as *mut u8, layout);
        }
    }

    #[test]
    fn huge_mapping_suffix_uses_raw_mapping_base() {
        let mut segment_storage = core::mem::MaybeUninit::<Segment>::uninit();
        let segment = segment_storage.as_mut_ptr();
        let raw = 0x1000usize as *mut u8;
        unsafe {
            Segment::initialize(segment, raw, 0);
            (*segment).pages[0].block_size = 0x4000;
        }

        let user_ptr = 0x2800usize as *const u8;
        let suffix = unsafe { (*segment).huge_mapping_suffix_from(user_ptr) };

        assert_eq!(
            suffix, 0x2800,
            "huge usable suffix must be raw_alloc_ptr + block_size - user_ptr"
        );
    }
}
