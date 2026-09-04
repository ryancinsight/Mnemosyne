//! Page-local block allocation: bump/free-list pop and free-list initialization.
//!
//! These `impl Page` methods carve blocks out of a page — popping from the
//! page-local free list or the lazy bump range, and building the (optionally
//! randomized, optionally encrypted) initial free list — split from the page
//! type definition by Separation of Concerns.

use crate::abort::abort_on_corruption;
use crate::types::{Block, Page, Segment};
use core::ptr::NonNull;

#[inline(always)]
const fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

impl Page {
    /// Tries to carve the next block from a page's uninitialized bump range.
    ///
    /// Keeping this raw-pointer operation in the core page owner lets local
    /// allocators use the same lazy-allocation contract without reconstructing
    /// a `Page` reference or calling the free-list path for every block.
    ///
    /// # Safety
    ///
    /// `page` must identify a live, exclusively owned page whose metadata has
    /// been initialized. The page's `block_size`, `size_class`, `page_index`,
    /// and `initialized_blocks` fields must describe a valid page layout.
    #[inline(always)]
    pub unsafe fn try_pop_bump_block(page: *mut Page) -> Option<NonNull<Block>> {
        // SAFETY: the caller guarantees that `page` is a live initialized page
        // exclusively owned by this allocation path.
        let (free, initialized, block_size, size_class, page_index) = unsafe {
            (
                (*page).free,
                (*page).initialized_blocks,
                (*page).block_size,
                (*page).size_class,
                (*page).page_index as usize,
            )
        };
        if free.is_some() {
            return None;
        }

        let max_blocks = crate::size_class::class_to_max_blocks(size_class as usize);
        if initialized >= max_blocks {
            return None;
        }

        // SAFETY: `initialized < max_blocks` means the next block remains
        // inside this page, and the page metadata is exclusively owned here.
        unsafe { (*page).initialized_blocks = initialized + 1 };

        let segment_addr = page.addr() & !(crate::constants::SEGMENT_SIZE - 1);
        let segment = page.map_addr(|_| segment_addr).cast::<Segment>();
        // SAFETY: `page` retains its parent segment mapping's provenance and
        // the initialized page index selects an in-range physical page.
        let page_start = unsafe { Self::page_start_in_segment(segment, page_index) };
        // SAFETY: the bump-range invariant established above bounds this block
        // offset within the page and preserves the block's required alignment.
        let block_ptr = unsafe { page_start.add(initialized * block_size) } as *mut Block;
        // SAFETY: `page_start` is non-null and the in-bounds offset keeps the
        // returned block pointer non-null.
        Some(unsafe { NonNull::new_unchecked(block_ptr) })
    }

    /// Pops a block from the page's local free list, using lazy/bump allocation if necessary.
    ///
    /// # Safety
    ///
    /// `page` must identify a live, exclusively owned page projected from its
    /// complete segment mapping. The page must have free blocks or
    /// uninitialized blocks remaining.
    #[inline(always)]
    pub unsafe fn pop_block<P: crate::policy::AllocPolicy>(page: *mut Self) -> NonNull<Block> {
        // SAFETY: forwarded from `pop_block`'s contract — `page` is a live,
        // exclusively-owned page with free or uninitialized blocks remaining.
        if let Some(block) = unsafe { Self::try_pop_bump_block(page) } {
            block
        // SAFETY: same contract — reading `free` from an exclusively-owned page.
        } else if let Some(block) = unsafe { (*page).free } {
            let block_addr = block.as_ptr() as usize;
            let page_addr = page.addr();
            let segment_addr = page_addr & !(crate::constants::SEGMENT_SIZE - 1);
            // SAFETY: `page` is exclusively owned; its `page_index` is initialized.
            let page_start = segment_addr
                + (unsafe { (*page).page_index as usize } << crate::constants::PAGE_SHIFT);
            // SAFETY: `page` is exclusively owned; `block_size` is initialized.
            let block_size = unsafe { (*page).block_size };
            if block_addr < page_start
                || block_addr + block_size > page_start + crate::constants::PAGE_SIZE
                || (block_addr & (crate::constants::MIN_BLOCK_SIZE - 1)) != 0
            {
                abort_on_corruption(
                    "pop_block found a free-list node outside its page or misaligned",
                );
            }
            let segment = page.map_addr(|_| segment_addr).cast::<Segment>();
            let page_index = unsafe { (*page).page_index as usize };
            // SAFETY: `page` retains the parent mapping provenance and its
            // initialized index is in range, satisfying `cookie_for`.
            let cookie = unsafe { Segment::cookie_for::<P>(segment, page_index) };
            // SAFETY: `block` came from `self.free`, the page-local free list
            // whose nodes are validated above to lie within the page and be
            // `MIN_BLOCK_SIZE`-aligned, so `block.as_ptr()` is a valid, aligned
            // `Block` exclusively owned by this thread; reading its encoded
            // next-link with the matching `cookie` is sound.
            unsafe { (*page).free = (*block.as_ptr()).get_next::<P>(cookie) };
            block
        } else {
            abort_on_corruption("pop_block called on an exhausted page");
        }
    }

    /// Builds the page's free list, addressing the page through its segment.
    ///
    /// # Safety
    ///
    /// `segment` must be a valid segment header, `page_index` must be in range
    /// of its `pages` array, and `page_start` must point to the start of that
    /// page and be valid for `PAGE_SIZE` reads and writes.
    pub unsafe fn initialize_free_list_in_segment<P: crate::policy::AllocPolicy>(
        segment: *mut Segment,
        page_index: usize,
        page_start: *mut u8,
        random_value: u64,
    ) {
        // Addressed by segment rather than `&mut self`: this function resets the
        // allocation count and reads the segment's free-list cookie, both of
        // which reach the parent header. A page borrow held across those
        // accesses is invalidated by them (see the `reclaim` module's note), so
        // the page is projected out of `segment` and shares its provenance.
        debug_assert!(page_index < crate::constants::PAGES_PER_SEGMENT);
        // SAFETY: caller guarantees a valid header and in-range index, so the
        // projection stays inside the segment allocation.
        let page = unsafe { &raw mut (*segment).pages[page_index] };

        // SAFETY: valid header and in-range index, forwarded unchanged.
        unsafe { Self::set_alloc_count_in_segment(segment, page_index, 0) };
        if P::RANDOMIZE_ALLOCATION {
            // SAFETY: `page` addresses initialized page metadata in `segment`.
            let n = unsafe { (*page).max_blocks() };
            if n == 0 {
                // SAFETY: as above.
                unsafe {
                    (*page).initialized_blocks = 0;
                    (*page).free = None;
                }
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

            // SAFETY: `segment` is the valid parent header and `page_index` is
            // in range, satisfying `cookie_for`'s contract. This read shares the
            // page projection's provenance, so neither invalidates the other.
            let cookie = unsafe { Segment::cookie_for::<P>(segment, page_index) };

            // SAFETY: `page` addresses initialized page metadata in `segment`.
            let block_size = unsafe { (*page).block_size };
            let mut prev_block: Option<NonNull<Block>> = None;
            let mut current_idx = start;
            for _ in 0..n {
                // SAFETY: `current_idx < n = max_blocks()` (the loop runs `n`
                // times over a permutation of `0..n`), and the function's
                // `# Safety` contract guarantees `page_start` is valid for the
                // full `PAGE_SIZE`, so `current_idx * block_size` is an in-bounds
                // byte offset of a block that fits within the page.
                let block_ptr = unsafe { page_start.add(current_idx * block_size) } as *mut Block;
                // SAFETY: `page_start` is non-null and the in-bounds offset above
                // keeps `block_ptr` non-null, upholding the `NonNull` invariant.
                let block = unsafe { NonNull::new_unchecked(block_ptr) };
                if let Some(prev) = prev_block {
                    // SAFETY: `prev` is a `block` produced by a previous iteration
                    // — an in-bounds, page-resident `Block` this thread owns
                    // exclusively while initializing the fresh page — so writing
                    // its next-link is sound.
                    unsafe {
                        (*prev.as_ptr()).set_next::<P>(Some(block), cookie);
                    }
                } else {
                    // SAFETY: `page` addresses initialized page metadata.
                    unsafe { (*page).free = Some(block) };
                }
                prev_block = Some(block);
                current_idx = (current_idx + stride) % n;
            }
            if let Some(prev) = prev_block {
                // SAFETY: `prev` is the last `block` constructed above — an
                // in-bounds, page-resident `Block` exclusively owned during
                // initialization — so terminating its next-link with `None` is a
                // valid write.
                unsafe {
                    (*prev.as_ptr()).set_next::<P>(None, cookie);
                }
            }
            // SAFETY: `page` addresses initialized page metadata.
            unsafe { (*page).initialized_blocks = n };
        } else {
            // SAFETY: as above.
            unsafe {
                (*page).initialized_blocks = 0;
                (*page).free = None;
            }
        }
    }
}
