//! Shared page-state helpers for the free path.

//! Extracted from `free.rs` so the hot free entry points and the shared
//! page-list primitives stay individually readable; these two are consumed
//! cross-module by `realloc.rs` and `local_alloc::segment::reclaim`.

use core::ptr::NonNull;

use mnemosyne_core::types::{Block, Page};

/// Returns true when `page` is the single page linked in the active list rooted
/// at `active_head` (it is the head and has no successor).
///
/// This is the "do not strand the last active page in the empty list" guard
/// shared by the local-free active→empty transition and the defragmentation
/// sweep; keeping it linked as an active page lets the next allocation of this
/// class reuse it without a cold refill.
///
/// # Safety
///
/// `active_head` must be the head of an intrusive active-page list owned by the
/// calling thread, and `page` must be a live page pointer from that thread's
/// allocator, so the head dereference is a valid, unaliased read.
#[inline(always)]
pub(crate) unsafe fn is_sole_active_page(
    active_head: Option<NonNull<Page>>,
    page: *const Page,
) -> bool {
    active_head.is_some_and(|head| {
        // SAFETY: `head` is a live, owner-exclusive page pointer per the caller's
        // contract; reading `next_page` is a valid shared read.
        core::ptr::eq(head.as_ptr(), page) && unsafe { (*head.as_ptr()).next_page.is_none() }
    })
}

/// Commits an in-place block free onto a page that keeps its list membership:
/// links `block` at the front of the page-local free list and decrements the
/// live count without touching any page-list or segment-occupancy state.
///
/// This is the hot "page stays active / is the current slicing segment" arm,
/// shared by `thread_free` and the small-realloc old-block free. Because the
/// page neither empties (its count stays `>= 1`) nor changes list, the plain
/// `alloc_count` decrement is correct: `decrement_alloc_count_for_segment` would
/// touch the occupancy mask only on the `count == 0` transition, which does not
/// occur here.
///
/// # Safety
///
/// `block` must be a live, non-null block previously allocated in `page` (its
/// double-free/underflow guards must already have passed), `page_free`/`cookie`
/// must be `page`'s current free-list head and encryption cookie, and
/// `page_alloc_count` must be `page.alloc_count` (`>= 1`).
#[inline(always)]
pub(crate) unsafe fn commit_in_place_free(
    block: *mut Block,
    page: *mut Page,
    page_free: Option<NonNull<Block>>,
    cookie: usize,
    encrypted: bool,
    page_alloc_count: usize,
) {
    // Backward-edge canary check: abort on double-free under encryption.
    // Uses the segment's actual `free_list_encrypted` flag rather than the
    // policy flag so mixed-policy usage never triggers a false positive.
    if encrypted {
        // SAFETY: `block` is a live, in-bounds block per the caller's contract;
        // the canary slot at `block + size_of::<Block>()` lies within the
        // minimum-block-size allocation.
        if unsafe { Block::check_double_free(block, cookie) } {
            std::process::abort();
        }
    }
    // SAFETY: `block` is a live, non-null block owned by `page` per the caller's
    // contract; the free-list head mutation stays inside that page.
    unsafe {
        (*block).set_next_dynamic(page_free, encrypted, cookie);
        (*page).free = Some(NonNull::new_unchecked(block));
        (*page).alloc_count = page_alloc_count - 1;
    }
    // Write the backward-edge canary after publishing the block onto the free list.
    if encrypted {
        // SAFETY: same contract as `check_double_free` above.
        unsafe { Block::write_free_canary(block, cookie) };
    }
}
