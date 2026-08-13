use core::ptr::NonNull;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Block, Page};

/// Pops the head block from an initialized page-local free list.
///
/// # Why these take `*mut Page` rather than `&mut Page`
///
/// The allocator's page lists carry addresses and refresh exposed provenance at
/// each use (see `page::access`), so a page pointer here is already a wildcard.
/// Turning one into `&mut Page` mints a `Unique` tag, and the page's own
/// segment accesses — the occupancy mask, `is_current`, the free-list cookie —
/// then have to pop that tag, which is Undefined Behavior. Staying on raw
/// pointers keeps every access in this path on the same wildcard footing.
///
/// # Safety
///
/// `page` must identify a live page whose `free` list is `Some`; callers
/// establish this through an existing local free list, a successful
/// `Page::reclaim_thread_free_in_segment`, or `initialize_free_list_in_segment`.
#[inline(always)]
pub(crate) unsafe fn pop_page_free_block<P: AllocPolicy>(page: *mut Page) -> NonNull<Block> {
    // SAFETY: caller guarantees `page` is live with a non-empty free list.
    unsafe { (*page).pop_block::<P>() }
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
    page: *mut Page,
) -> Option<NonNull<Block>> {
    // SAFETY: caller guarantees `page` identifies a live page it owns.
    unsafe {
        if (*page).free.is_none() && (*page).initialized_blocks >= (*page).max_blocks() {
            return None;
        }
        let block = (*page).pop_block::<P>();
        let segment = (*page).parent_segment();
        let page_index = (*page).index_in_segment();
        Page::increment_alloc_count_in_segment(segment, page_index);
        Some(block)
    }
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
/// Same contract as `Page::reclaim_thread_free_in_segment`: the page must
/// belong to the allocator context performing the reconciliation and every
/// block in `page.thread_free` must belong to this page.
#[inline(always)]
pub(crate) unsafe fn try_reclaim_and_allocate<P: AllocPolicy>(
    page: *mut Page,
    reclaim_sink: &mut usize,
) -> Option<NonNull<Block>> {
    // SAFETY: caller guarantees `page` identifies a live page it owns.
    let (segment, page_index) = unsafe {
        if (*page).thread_free.is_empty() {
            return None;
        }
        ((*page).parent_segment(), (*page).index_in_segment())
    };

    // SAFETY: `parent_segment`/`index_in_segment` name this page's parent
    // header and its own in-range index.
    let reclaimed = unsafe {
        Page::reclaim_thread_free_in_segment(segment, page_index, P::ENABLE_FREE_LIST_ENCRYPTION)
    };
    if reclaimed == 0 {
        return None;
    }
    *reclaim_sink += reclaimed;
    // SAFETY: a nonzero reclaim count guarantees the drained chain is now
    // linked onto the page's local free list.
    let block = unsafe { try_allocate_page_local::<P>(page) }
        .expect("invariant: reclaimed remote frees populate the page-local free list");
    Some(block)
}
