//! Page-local block allocation: bump/free-list pop and free-list initialization.
//!
//! These `impl Page` methods carve blocks out of a page — popping from the
//! page-local free list or the lazy bump range, and building the (optionally
//! randomized, optionally encrypted) initial free list — split from the page
//! type definition by Separation of Concerns.

use crate::abort::abort_on_corruption;
use crate::types::{Block, Page};
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
    /// Pops a block from the page's local free list, using lazy/bump allocation if necessary.
    ///
    /// # Safety
    ///
    /// The page must have free blocks or uninitialized blocks remaining.
    #[inline(always)]
    pub unsafe fn pop_block<P: crate::policy::AllocPolicy>(&mut self) -> NonNull<Block> {
        if let Some(block) = self.free {
            let block_addr = block.as_ptr() as usize;
            let page_start = self.page_start() as usize;
            if block_addr < page_start
                || block_addr + self.block_size > page_start + crate::constants::PAGE_SIZE
                || (block_addr & (crate::constants::MIN_BLOCK_SIZE - 1)) != 0
            {
                abort_on_corruption(
                    "pop_block found a free-list node outside its page or misaligned",
                );
            }
            // SAFETY: `parent_segment` returns `self`'s valid parent header and
            // `index_in_segment` is this page's in-range index, satisfying
            // `cookie_for`'s contract.
            let cookie =
                unsafe { (*self.parent_segment()).cookie_for::<P>(self.index_in_segment()) };
            // SAFETY: `block` came from `self.free`, the page-local free list
            // whose nodes are validated above to lie within the page and be
            // `MIN_BLOCK_SIZE`-aligned, so `block.as_ptr()` is a valid, aligned
            // `Block` exclusively owned by this thread; reading its encoded
            // next-link with the matching `cookie` is sound.
            self.free = unsafe { (*block.as_ptr()).get_next::<P>(cookie) };
            block
        } else if self.initialized_blocks < self.max_blocks() {
            let idx = self.initialized_blocks;
            self.initialized_blocks += 1;
            let page_start = self.page_start();
            // SAFETY: bump path — `idx = initialized_blocks < max_blocks()`, so
            // `idx * block_size` is a byte offset of a block that fits entirely
            // within this page's `PAGE_SIZE` region starting at `page_start`,
            // hence in bounds of the page object.
            let block_ptr = unsafe { page_start.add(idx * self.block_size) } as *mut Block;
            // SAFETY: `page_start` is a non-null page base and the in-bounds
            // offset above keeps `block_ptr` non-null, so the `NonNull`
            // invariant (pointer is non-null) holds.
            unsafe { NonNull::new_unchecked(block_ptr) }
        } else {
            // SAFETY: the function's `# Safety` contract requires the page to
            // have free blocks or uninitialized blocks remaining; both prior
            // arms are exhausted only when neither holds, which the caller
            // guarantees cannot occur, so this branch is genuinely unreachable.
            unsafe { core::hint::unreachable_unchecked() }
        }
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
        // SAFETY: `set_alloc_count` recovers the parent segment by masking
        // `self`'s address to `SEGMENT_SIZE`; a `Page` is always embedded in its
        // segment's `pages` array, so that header is valid — the precondition of
        // `set_alloc_count` is met.
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

            // SAFETY: `parent_segment` returns `self`'s valid parent header and
            // `index_in_segment` is this page's in-range index, satisfying
            // `cookie_for`'s contract.
            let cookie =
                unsafe { (*self.parent_segment()).cookie_for::<P>(self.index_in_segment()) };

            let block_size = self.block_size;
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
                    self.free = Some(block);
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
            self.initialized_blocks = n;
        } else {
            self.initialized_blocks = 0;
            self.free = None;
        }
    }
}
