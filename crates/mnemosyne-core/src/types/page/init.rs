//! Page-local block allocation: bump/free-list pop and free-list initialization.
//!
//! These `impl Page` methods carve blocks out of a page — popping from the
//! page-local free list or the lazy bump range, and building the (optionally
//! randomized, optionally encrypted) initial free list — split from the page
//! type definition by Separation of Concerns.

use crate::abort::abort_on_corruption;
use crate::constants::PAGE_SIZE;
use crate::types::page::assert_block_in_page;
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

/// Tries to carve the next block from a page's uninitialized bump range.
///
/// Non-generic: bump allocation needs only page-metadata arithmetic with no
/// policy const. Removing the `<P: AllocPolicy>` type parameter means this
/// compiles once rather than once per policy — the body is identical across
/// all policies.
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
    let (free, secondary, initialized, block_size, size_class, page_index) = unsafe {
        (
            (*page).free,
            (*page).secondary_free,
            (*page).initialized_blocks,
            (*page).block_size,
            (*page).size_class,
            (*page).page_index as usize,
        )
    };
    if free.is_some() || secondary.is_some() {
        return None;
    }

    let max_blocks = crate::size_class::class_to_max_blocks(size_class as usize);
    if initialized as usize >= max_blocks {
        return None;
    }

    // SAFETY: `initialized < max_blocks` means the next block remains
    // inside this page, and the page metadata is exclusively owned here.
    unsafe { (*page).initialized_blocks = initialized + 1 };

    let segment_addr = page.addr() & !(crate::constants::SEGMENT_SIZE - 1);
    let segment = page.map_addr(|_| segment_addr).cast::<Segment>();
    // SAFETY: `page` retains its parent segment mapping's provenance and
    // the initialized page index selects an in-range physical page.
    let page_start = unsafe { Page::page_start_in_segment(segment, page_index) };
    // SAFETY: the bump-range invariant established above bounds this block
    // offset within the page and preserves the block's required alignment.
    let block_ptr =
        unsafe { page_start.add(initialized as usize * block_size as usize) } as *mut Block;
    // SAFETY: `page_start` is non-null and the in-bounds offset keeps the
    // returned block pointer non-null.
    Some(unsafe { NonNull::new_unchecked(block_ptr) })
}

impl Page {
    /// Pops a block from the page's local free list, using lazy/bump allocation if necessary.
    ///
    /// # Safety
    ///
    /// `page` must identify a live, exclusively owned page projected from its
    /// complete segment mapping. The page must have free blocks or
    /// uninitialized blocks remaining.
    #[inline(always)]
    pub unsafe fn pop_block<P: crate::policy::AllocPolicy>(page: *mut Self) -> NonNull<Block> {
        // SAFETY: forwarded — same contract.
        unsafe {
            Self::pop_block_dynamic(page, P::ENABLE_FREE_LIST_ENCRYPTION, P::RANDOMIZE_ALLOCATION)
        }
    }

    /// Non-generic SSOT for [`pop_block`].
    ///
    /// All P:: constants are replaced with plain `bool` parameters so:
    /// - StandardPolicy `(encrypt=false, randomize=false)` and SecurePolicy
    ///   `(encrypt=false, randomize=true)` both call `pop_block_dynamic` with
    ///   their own bools, sharing the code path without the generic overhead.
    /// - Callers on the cold path (`alloc_cold_raw`) can pass runtime bools
    ///   and avoid a per-policy instantiation of the cold body.
    ///
    /// # Safety
    ///
    /// Same contract as [`pop_block`].
    #[inline(always)]
    pub unsafe fn pop_block_dynamic(
        page: *mut Self,
        enable_encryption: bool,
        randomize: bool,
    ) -> NonNull<Block> {
        // SAFETY: forwarded from `pop_block`'s contract — `page` is a live,
        // exclusively-owned page with free or uninitialized blocks remaining.
        if let Some(block) = unsafe { try_pop_bump_block(page) } {
            return block;
        }
        let (head, use_secondary) = unsafe {
            Self::choose_free_head(page, (*page).alloc_count as usize, randomize)
        };
        let Some(block) = head else {
            abort_on_corruption("pop_block called on an exhausted page");
        };
        let block_addr = block.as_ptr() as usize;
        let page_addr = page.addr();
        let segment_addr = page_addr & !(crate::constants::SEGMENT_SIZE - 1);
        // SAFETY: `page` is exclusively owned; its `page_index` is initialized.
        let page_start = segment_addr
            + (unsafe { (*page).page_index as usize } << crate::constants::PAGE_SHIFT);
        // SAFETY: `page` is exclusively owned; `block_size` is initialized.
        let block_size = unsafe { (*page).block_size } as usize;
        assert_block_in_page(
            block_addr,
            block_size,
            page_start,
            page_start + PAGE_SIZE,
            "pop_block found a free-list node outside its page or misaligned",
        );
        let segment = page.map_addr(|_| segment_addr).cast::<Segment>();
        let page_index = unsafe { (*page).page_index as usize };
        // SAFETY: `page` retains the parent mapping provenance and its
        // initialized index is in range, satisfying `cookie_for_dynamic`.
        let cookie =
            unsafe { Segment::cookie_for_dynamic(segment, enable_encryption, page_index) };
        // SAFETY: `block` came from one of the page-local free chains, whose
        // nodes are validated above to lie within the page and be
        // `MIN_BLOCK_SIZE`-aligned, so `block.as_ptr()` is a valid, aligned
        // `Block` exclusively owned by this thread; reading its encoded
        // next-link with the matching `cookie` is sound.
        let next = unsafe { (*block.as_ptr()).get_next_dynamic(enable_encryption, cookie) };
        let next = match next {
            Some(next) => {
                let next_addr = next.as_ptr() as usize;
                let page_end = page_start + crate::constants::PAGE_SIZE;
                let next_block_size = unsafe { (*page).block_size } as usize;
                assert_block_in_page(
                    next_addr,
                    next_block_size,
                    page_start,
                    page_end,
                    "pop_block found a corrupted free-list next pointer outside its page",
                );
                Some(next)
            }
            None => None,
        };
        // SAFETY: `page` is this thread's own live page header for the
        // whole pop, and `next` was decoded from the same list under the
        // same cookie, so the head it replaces stays decodable.
        if use_secondary {
            unsafe { (*page).secondary_free = next };
        } else {
            unsafe { (*page).free = next };
        }
        // Clear the backward-edge canary on the block being handed out.
        if enable_encryption {
            // SAFETY: `block.as_ptr()` is valid, MIN_BLOCK_SIZE-aligned per
            // the bounds check above; the canary slot fits within that minimum.
            unsafe { Block::clear_free_canary(block.as_ptr()) };
        }
        block
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
        // SAFETY: forwarded — same contract.
        unsafe {
            Self::initialize_free_list_in_segment_dynamic(
                segment,
                page_index,
                page_start,
                random_value,
                P::ENABLE_FREE_LIST_ENCRYPTION,
                P::RANDOMIZE_ALLOCATION,
            )
        }
    }

    /// Non-generic SSOT for [`initialize_free_list_in_segment`].
    ///
    /// # Safety
    ///
    /// Same contract as `initialize_free_list_in_segment`.
    pub unsafe fn initialize_free_list_in_segment_dynamic(
        segment: *mut Segment,
        page_index: usize,
        page_start: *mut u8,
        random_value: u64,
        enable_encryption: bool,
        randomize: bool,
    ) {
        debug_assert!(page_index < crate::constants::PAGES_PER_SEGMENT);
        // SAFETY: caller guarantees a valid header and in-range index.
        let page = unsafe { &raw mut (*segment).pages[page_index] };

        // SAFETY: valid header and in-range index, forwarded unchanged.
        unsafe { Self::set_alloc_count_in_segment(segment, page_index, 0) };
        if randomize {
            // SAFETY: `page` addresses initialized page metadata in `segment`.
            let n = unsafe { (*page).max_blocks() };
            if n == 0 {
                // SAFETY: as above.
                unsafe {
                    (*page).initialized_blocks = 0;
                    (*page).free = None;
                    (*page).secondary_free = None;
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

            // SAFETY: `segment`/`page_index` satisfy `cookie_for_dynamic`'s contract.
            let cookie =
                unsafe { Segment::cookie_for_dynamic(segment, enable_encryption, page_index) };

            // SAFETY: `page` addresses initialized page metadata in `segment`.
            let block_size = unsafe { (*page).block_size } as usize;
            let mut primary_prev: Option<NonNull<Block>> = None;
            let mut secondary_prev: Option<NonNull<Block>> = None;
            let mut current_idx = start;
            for _ in 0..n {
                // SAFETY: `current_idx < n = max_blocks()` and `page_start`
                // is valid for `PAGE_SIZE` per the caller's contract.
                let block_ptr = unsafe { page_start.add(current_idx * block_size) } as *mut Block;
                // SAFETY: in-bounds offset keeps `block_ptr` non-null.
                let block = unsafe { NonNull::new_unchecked(block_ptr) };
                if (current_idx ^ (random_value as usize >> 8)) & 1 == 0 {
                    if let Some(prev) = primary_prev {
                        // SAFETY: `prev` is a freshly carved block of this page.
                        unsafe {
                            (*prev.as_ptr()).set_next_dynamic(Some(block), enable_encryption, cookie);
                        }
                    } else {
                        unsafe { (*page).free = Some(block) };
                    }
                    primary_prev = Some(block);
                } else {
                    if let Some(prev) = secondary_prev {
                        // SAFETY: `prev` is a freshly carved block of this page.
                        unsafe {
                            (*prev.as_ptr()).set_next_dynamic(Some(block), enable_encryption, cookie);
                        }
                    } else {
                        unsafe { (*page).secondary_free = Some(block) };
                    }
                    secondary_prev = Some(block);
                }
                current_idx = (current_idx + stride) % n;
            }
            // Terminate both chains.
            if let Some(prev) = primary_prev {
                // SAFETY: `prev` is a freshly carved block of this page with
                // the page's own key; terminating the list is an exclusive write.
                unsafe {
                    (*prev.as_ptr()).set_next_dynamic(None, enable_encryption, cookie);
                }
            }
            if let Some(prev) = secondary_prev {
                // SAFETY: same as the primary termination above.
                unsafe {
                    (*prev.as_ptr()).set_next_dynamic(None, enable_encryption, cookie);
                }
            }
            // SAFETY: `page` addresses initialized page metadata.
            unsafe { (*page).initialized_blocks = n as u32 };
        } else {
            // SAFETY: as above.
            unsafe {
                (*page).initialized_blocks = 0;
                (*page).free = None;
                (*page).secondary_free = None;
            }
        }
    }
}
