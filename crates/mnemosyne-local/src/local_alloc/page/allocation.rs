use core::ptr::NonNull;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Block, Page};

/// Pops the head block from an initialized page-local free list.
///
/// # Safety
///
/// `page.free` must be `Some`; callers establish this through an existing
/// local free list, a successful `Page::reclaim_thread_free`, or
/// `Page::initialize_free_list`.
#[inline(always)]
pub(crate) unsafe fn pop_page_free_block<P: AllocPolicy>(page: &mut Page) -> NonNull<Block> {
    unsafe { page.pop_block::<P>() }
}

/// Allocates one block from a page-local free list or from that page's lazy
/// bump range.
///
/// Returns `None` when the page has no local free block and no uninitialized
/// block remaining.
///
/// # Safety
///
/// The caller must own `page` through the current thread allocator and must
/// ensure that any decoded free-list links use policy `P`.
#[inline(always)]
pub(crate) unsafe fn try_allocate_page_local<P: AllocPolicy>(
    page: &mut Page,
) -> Option<NonNull<Block>> {
    if page.free.is_none() && page.initialized_blocks >= page.max_blocks() {
        return None;
    }
    let block = unsafe { page.pop_block::<P>() };
    unsafe { page.increment_alloc_count() };
    Some(block)
}

/// Reclaims any pending cross-thread frees on `page` and, if reclamation
/// added blocks to the local free list, pops one block and increments the
/// page's `alloc_count`.
///
/// Returns the popped block when reclamation succeeded, or `None` when
/// `page.thread_free` was empty. Any reclaimed block count is added to
/// `reclaim_sink`, the owning allocator's per-thread `cross_thread_reclaimed`
/// counter, so the reclaim path never touches the process-global atomic.
///
/// # Safety
///
/// Same contract as `Page::reclaim_thread_free`: the page must belong to
/// the allocator context performing the reconciliation and every block in
/// `page.thread_free` must belong to this page.
#[inline(always)]
pub(crate) unsafe fn try_reclaim_and_allocate<P: AllocPolicy>(
    page: &mut Page,
    reclaim_sink: &mut usize,
) -> Option<NonNull<Block>> {
    if page.thread_free.is_empty() {
        return None;
    }
    let segment = page.parent_segment();
    let page_index = page.index_in_segment();

    let reclaimed = unsafe {
        page.reclaim_thread_free_dynamic_for_segment(
            P::ENABLE_FREE_LIST_ENCRYPTION,
            segment,
            page_index,
        )
    };
    if reclaimed == 0 {
        return None;
    }
    *reclaim_sink += reclaimed;
    // Safety: `reclaim_thread_free` returning a nonzero count guarantees
    // that the drained chain is now linked onto `page.free`.
    let block = unsafe { try_allocate_page_local::<P>(page) }
        .expect("invariant: reclaimed remote frees populate the page-local free list");
    Some(block)
}
