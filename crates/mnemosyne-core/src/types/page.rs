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
    pub size_class: u32,
    /// Current list state of this page (0=None, 1=Active, 2=Full, 3=Empty).
    pub list_state: u32,
}

unsafe impl Send for Page {}
unsafe impl Sync for Page {}

#[inline]
fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

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
}

impl Default for Page {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Page {
    /// Recovers this page's index within its parent segment's `pages` array
    /// from the page's own metadata address, in O(1).
    #[inline]
    pub fn index_in_segment(&self) -> usize {
        let self_addr = self as *const Page as usize;
        let segment_addr = self_addr & !(crate::constants::SEGMENT_SIZE - 1);
        let pages_base = segment_addr + core::mem::offset_of!(Segment, pages);
        (self_addr - pages_base) / core::mem::size_of::<Page>()
    }

    /// Sets the active allocation count for this page and updates the parent
    /// segment's hierarchical `page_occupied_mask` bit vector in-place.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that the parent segment is a valid Segment mapping.
    #[inline(always)]
    pub unsafe fn set_alloc_count(&mut self, count: usize) {
        let old = self.alloc_count;
        if old == count {
            return;
        }
        self.alloc_count = count;
        if (old == 0) != (count == 0) {
            let self_addr = self as *mut Page as usize;
            let segment_addr = self_addr & !(crate::constants::SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;
            let idx = self.index_in_segment();
            unsafe {
                if count > 0 {
                    (*segment).page_occupied_mask |= 1 << idx;
                } else {
                    (*segment).page_occupied_mask &= !(1 << idx);
                }
            }
        }
    }

    /// Returns the physical start address of this page in memory.
    #[inline(always)]
    pub fn page_start(&self) -> *mut u8 {
        let self_addr = self as *const Page as usize;
        let segment_addr = self_addr & !(crate::constants::SEGMENT_SIZE - 1);
        let offset = self_addr - segment_addr - core::mem::offset_of!(Segment, pages);
        let page_offset = offset
            << (crate::constants::PAGE_SHIFT
                - core::mem::size_of::<Page>().trailing_zeros() as usize);
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
        unsafe { self.set_alloc_count(self.alloc_count - count) };

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
        page_start: *mut u8,
        random_value: u64,
    ) {
        unsafe { self.set_alloc_count(0) };
        if P::RANDOMIZE_ALLOCATION {
            let n = self.max_blocks();
            if n == 0 {
                self.initialized_blocks = 0;
                self.free = None;
                return;
            }

            // Find a stride coprime to N.
            let mut stride = (random_value as usize) % n;
            if stride == 0 {
                stride = 1;
            }
            while gcd(stride, n) != 1 {
                stride = (stride + 1) % n;
                if stride == 0 {
                    stride = 1;
                }
            }

            // Start index
            let start = (random_value >> 16) as usize % n;

            let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
                let self_addr = self as *const Page as usize;
                let segment_addr = self_addr & !(crate::constants::SEGMENT_SIZE - 1);
                let segment = segment_addr as *mut Segment;
                let page_index = self.index_in_segment();
                unsafe { (*segment).keys[page_index] }
            } else {
                0
            };

            let block_size = self.block_size;
            let mut prev_block: Option<NonNull<Block>> = None;
            let mut current_idx = start;
            for _ in 0..n {
                let block_ptr = unsafe { page_start.add(current_idx * block_size) } as *mut Block;
                let block = unsafe { NonNull::new_unchecked(block_ptr) };
                if let Some(prev) = prev_block {
                    unsafe {
                        (*prev.as_ptr()).set_next::<P>(Some(block), cookie);
                    }
                } else {
                    self.free = Some(block);
                }
                prev_block = Some(block);
                current_idx = (current_idx + stride) % n;
            }
            if let Some(prev) = prev_block {
                unsafe {
                    (*prev.as_ptr()).set_next::<P>(None, cookie);
                }
            }
            self.initialized_blocks = n;
        } else {
            self.initialized_blocks = 0;
            self.free = None;
        }
    }
}
