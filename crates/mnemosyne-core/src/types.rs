//! Core memory layout types: Block, Page, and Segment.

use crate::constants::{PAGE_SIZE, PAGES_PER_SEGMENT};
use crate::sync::AtomicFreeList;
use core::ptr::NonNull;

/// A node representing a free block.
///
/// Free blocks are stored inline within the allocated memory when free.
#[repr(transparent)]
pub struct Block {
    /// Pointer to the next free block.
    pub next: Option<NonNull<Block>>,
}

// Block is simple data, safe to send/sync as a memory representation.
unsafe impl Send for Block {}
unsafe impl Sync for Block {}

/// Metadata representing a page of memory.
///
/// Each page manages blocks of a single size class.
pub struct Page {
    /// Thread-local free list of blocks.
    pub free: Option<NonNull<Block>>,
    /// Thread-local list of recently freed blocks.
    pub local_free: Option<NonNull<Block>>,
    /// Lock-free list of blocks freed by other threads.
    pub thread_free: AtomicFreeList,
    /// Size of the blocks allocated in this page.
    pub block_size: usize,
    /// Number of active allocations.
    pub alloc_count: usize,
    /// Maximum number of blocks in this page.
    pub max_blocks: usize,
    /// Pointer to the next page in the thread-local size class list.
    pub next_page: Option<NonNull<Page>>,
    /// Pointer to the parent segment containing this page.
    pub segment: Option<NonNull<Segment>>,
    /// The page index inside its parent segment.
    pub page_index: usize,
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
pub struct SegmentOwner(*mut core::ffi::c_void);

impl SegmentOwner {
    /// No thread currently owns this segment.
    pub const NONE: Self = Self(core::ptr::null_mut());

    /// Builds an owner token from an allocator pointer.
    #[inline(always)]
    pub fn from_ptr<T>(ptr: *mut T) -> Self {
        Self(ptr.cast())
    }

    /// Returns true when this token identifies `ptr`.
    #[inline(always)]
    pub fn matches<T>(self, ptr: *mut T) -> bool {
        self.0 == ptr.cast()
    }
}

unsafe impl Send for SegmentOwner {}
unsafe impl Sync for SegmentOwner {}

impl Page {
    /// Creates a new uninitialized `Page`.
    pub const fn new(page_index: usize) -> Self {
        Self {
            free: None,
            local_free: None,
            thread_free: AtomicFreeList::new(),
            block_size: 0,
            alloc_count: 0,
            max_blocks: 0,
            next_page: None,
            segment: None,
            page_index,
        }
    }

    /// Checks if the page has no active allocations.
    pub fn is_empty(&self) -> bool {
        self.alloc_count == 0
    }

    /// Atomically drains cross-thread frees into the page-local free list.
    ///
    /// Returns the number of reclaimed blocks. The caller owns the surrounding
    /// allocator metadata update and may use the count for telemetry.
    ///
    /// # Safety
    ///
    /// The page must belong to the allocator context currently reconciling its
    /// metadata, and every block in `thread_free` must belong to this page.
    #[inline]
    pub unsafe fn reclaim_thread_free(&mut self) -> usize {
        let Some(block) = self.thread_free.pop_all() else {
            return 0;
        };

        let mut count = 0;
        let mut current = Some(block);
        let mut last = block;
        while let Some(node) = current {
            count += 1;
            last = node;
            current = unsafe { (*node.as_ptr()).next };
        }

        debug_assert!(self.alloc_count >= count);
        self.alloc_count -= count;

        unsafe {
            (*last.as_ptr()).next = self.free;
        }
        self.free = Some(block);
        count
    }

    /// Initializes a page's free list for a specific block size.
    ///
    /// # Safety
    ///
    /// The `page_start` pointer must point to the start of the 64KB page
    /// and must be valid for reads and writes of size `PAGE_SIZE`.
    pub unsafe fn initialize_free_list(&mut self, page_start: *mut u8) {
        let block_size = self.block_size;
        let num_blocks = PAGE_SIZE / block_size;
        self.max_blocks = num_blocks;
        self.alloc_count = 0;

        let mut prev: Option<NonNull<Block>> = None;
        for i in (0..num_blocks).rev() {
            let offset = i * block_size;
            // Safety: page_start is a valid pointer to the start of the 64KB page,
            // offset is within the bounds of this page, and block_ptr is aligned.
            // We write the next node to form the linked list.
            unsafe {
                let block_ptr = page_start.add(offset) as *mut Block;
                (*block_ptr).next = prev;
                prev = Some(NonNull::new_unchecked(block_ptr));
            }
        }
        self.free = prev;
        self.local_free = None;
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
    /// Pointer to the next segment owned by the same ThreadAllocator.
    pub next_owned_segment: *mut Segment,
    /// Pointer to the next free segment in the global pool.
    pub next_free_segment: *mut Segment,
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
    pub unsafe fn initialize(aligned_ptr: *mut Segment, raw_alloc_ptr: *mut u8) {
        // Safety: aligned_ptr must point to a valid, exclusive, aligned memory segment.
        // We initialize the segment fields and establish parent/child pointers safely.
        unsafe {
            let segment = &mut *aligned_ptr;
            segment.raw_alloc_ptr = raw_alloc_ptr;
            segment.owner = SegmentOwner::NONE;
            segment.next_owned_segment = core::ptr::null_mut();
            segment.next_free_segment = core::ptr::null_mut();

            for i in 0..PAGES_PER_SEGMENT {
                segment.pages[i] = Page::new(i);
                segment.pages[i].segment = Some(NonNull::new_unchecked(aligned_ptr));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::PAGE_SIZE;

    #[test]
    fn test_page_reclaim_thread_free() {
        let mut page = Page::new(1);
        page.block_size = 16;
        let mut storage = [0u8; PAGE_SIZE];

        unsafe {
            page.initialize_free_list(storage.as_mut_ptr());
        }

        let first = page.free.expect("initialized page has a free block");
        unsafe {
            page.free = (*first.as_ptr()).next;
        }
        page.alloc_count = 1;
        page.thread_free.push(first);

        let reclaimed = unsafe { page.reclaim_thread_free() };

        assert_eq!(reclaimed, 1);
        assert_eq!(page.alloc_count, 0);
        assert_eq!(page.free, Some(first));
        assert!(page.thread_free.is_empty());
    }
}
