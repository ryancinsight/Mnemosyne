use core::ptr::NonNull;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Block, Page, Segment, try_pop_bump_block};

/// Pops the head block from an initialized page-local free list.
///
/// # Safety
///
/// `page` must identify a live page whose `free` list is `Some`.
#[inline(always)]
pub(crate) unsafe fn pop_page_free_block<P: AllocPolicy>(page: *mut Page) -> NonNull<Block> {
    // SAFETY: caller guarantees `page` is live with a non-empty free list.
    unsafe {
        Page::pop_block_dynamic(page, P::ENABLE_FREE_LIST_ENCRYPTION, P::RANDOMIZE_ALLOCATION)
    }
}

/// Allocates one block from a page-local free list or from that page's lazy
/// bump range. Returns `None` when the page has no local free block and no
/// uninitialized block remaining.
///
/// # Safety
///
/// The caller must own `page` through the current thread allocator.
#[inline(always)]
pub(crate) unsafe fn try_allocate_page_local<P: AllocPolicy>(
    page: *mut Page,
) -> Option<NonNull<Block>> {
    // SAFETY: forwarded.
    unsafe {
        try_allocate_page_local_dynamic(page, P::ENABLE_FREE_LIST_ENCRYPTION, P::RANDOMIZE_ALLOCATION)
    }
}

/// Non-generic SSOT for `try_allocate_page_local`.
///
/// # Safety
///
/// Same contract as `try_allocate_page_local`.
#[inline(always)]
pub(crate) unsafe fn try_allocate_page_local_dynamic(
    page: *mut Page,
    enable_encryption: bool,
    randomize: bool,
) -> Option<NonNull<Block>> {
    // SAFETY: caller guarantees `page` identifies a live page it owns.
    unsafe {
        if (*page).free.is_none()
            && (*page).secondary_free.is_none()
            && (*page).initialized_blocks as usize >= (*page).max_blocks()
        {
            return None;
        }
        let block = if let Some(block) = try_pop_bump_block(page) {
            block
        } else if (*page).free.is_some() || (*page).secondary_free.is_some() {
            Page::pop_block_dynamic(page, enable_encryption, randomize)
        } else {
            return None;
        };
        if (*page).alloc_count == 0 {
            let segment = Page::parent_segment_of(page);
            let page_index = (*page).index_in_segment();
            Page::increment_alloc_count_in_segment(segment, page_index);
        } else {
            (*page).alloc_count = ((*page).alloc_count as usize + 1) as u32;
        }
        Some(block)
    }
}

/// Reclaims any pending cross-thread frees on `page` and pops one block.
///
/// # Safety
///
/// Same contract as `Page::reclaim_thread_free_in_segment`.
#[inline(always)]
pub(crate) unsafe fn try_reclaim_and_allocate<P: AllocPolicy>(
    page: *mut Page,
    reclaim_sink: &mut usize,
) -> Option<NonNull<Block>> {
    // SAFETY: forwarded.
    unsafe {
        try_reclaim_and_allocate_dynamic(
            page,
            reclaim_sink,
            P::ENABLE_FREE_LIST_ENCRYPTION,
            P::RANDOMIZE_ALLOCATION,
        )
    }
}

/// Non-generic SSOT for `try_reclaim_and_allocate`.
///
/// # Safety
///
/// Same contract as `try_reclaim_and_allocate`.
#[inline(always)]
pub(crate) unsafe fn try_reclaim_and_allocate_dynamic(
    page: *mut Page,
    reclaim_sink: &mut usize,
    enable_encryption: bool,
    randomize: bool,
) -> Option<NonNull<Block>> {
    // SAFETY: caller guarantees `page` identifies a live page it owns.
    let (segment, page_index) = unsafe {
        if (*page).thread_free.is_empty() {
            return None;
        }
        (Page::parent_segment_of(page), (*page).index_in_segment())
    };

    // SAFETY: `parent_segment`/`index_in_segment` name this page's parent header.
    let encrypted = unsafe { Segment::free_list_encrypted(segment) };
    let randomized = randomize && encrypted;
    let reclaimed =
        unsafe { Page::reclaim_thread_free_in_segment(segment, page_index, encrypted, randomized) };
    if reclaimed == 0 {
        return None;
    }
    *reclaim_sink += reclaimed;
    // SAFETY: a nonzero reclaim count guarantees the drained chain is now
    // linked onto the page's local free list.
    let block = unsafe { try_allocate_page_local_dynamic(page, enable_encryption, randomize) }
        .expect("invariant: reclaimed remote frees populate the page-local free list");
    Some(block)
}
