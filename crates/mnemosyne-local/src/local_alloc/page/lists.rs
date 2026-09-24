//! Non-generic intrusive page-list operations.
//!
//! All production code now calls the `_raw` helpers directly.
//! The exclusive-access invariant is enforced by `&mut ThreadAllocator<B>`
//! borrow at every call site — no phantom-brand token is needed.

use core::ptr::NonNull;
use mnemosyne_core::types::Page;

// ── Non-generic raw implementations ─────────────────────────────────────────

/// Pushes `raw_page` to the front of the intrusive doubly-linked page list
/// rooted at `head_slot`, setting `list_state` to `list_state`.
///
/// # Safety
///
/// `raw_page` must be exclusively owned by the calling allocator and must not
/// already be linked into `head_slot`.
#[inline(always)]
pub(crate) unsafe fn push_page_front_raw(
    raw_page: NonNull<Page>,
    head_slot: &mut Option<NonNull<Page>>,
    list_state: u8,
) {
    // SAFETY: `raw_page` is exclusively owned per the function's contract;
    // writing its link fields and the head slot does not alias any other borrow.
    unsafe {
        (*raw_page.as_ptr()).next_page = *head_slot;
        (*raw_page.as_ptr()).prev_page = None;
    }
    if let Some(head) = *head_slot {
        // SAFETY: the caller's exclusivity contract covers every page reachable
        // from `head_slot`; the head's prev-pointer write is unaliased.
        unsafe {
            (*head.as_ptr()).prev_page = Some(raw_page);
        }
    }
    *head_slot = Some(raw_page);
    // SAFETY: `raw_page` is exclusively owned; writing `list_state` is unaliased.
    unsafe { (*raw_page.as_ptr()).list_state = list_state };
    // SAFETY: `page_index` is initialized metadata in the exclusively-owned page.
    let page_index = unsafe { (*raw_page.as_ptr()).page_index };
    if page_index > 0 {
        // SAFETY: list nodes retain the segment mapping provenance; the parent
        // segment header and its `page_linked_mask` field are exclusively accessible.
        let segment = unsafe { Page::parent_segment_of(raw_page.as_ptr()) };
        unsafe {
            (*segment).page_linked_mask |= 1 << page_index;
        }
    }
}

/// Non-generic SSOT for unlinking a page from its intrusive doubly-linked list.
///
/// # Safety
///
/// `raw_page` must be exclusively owned and currently linked in the list
/// rooted at `head_slot`.
#[inline(always)]
pub(crate) unsafe fn unlink_page_from_list_raw(
    raw_page: NonNull<Page>,
    head_slot: &mut Option<NonNull<Page>>,
) {
    // SAFETY: `raw_page` is exclusively owned per the function's contract;
    // reading its link fields is unaliased.
    let next = unsafe { (*raw_page.as_ptr()).next_page };
    let prev = unsafe { (*raw_page.as_ptr()).prev_page };

    if let Some(prev_ptr) = prev {
        // SAFETY: the caller's exclusivity contract covers adjacent pages;
        // rewriting `next_page` on the predecessor is unaliased.
        unsafe { (*prev_ptr.as_ptr()).next_page = next };
    } else {
        *head_slot = next;
    }
    if let Some(next_ptr) = next {
        // SAFETY: same reasoning as the prev-pointer write above.
        unsafe { (*next_ptr.as_ptr()).prev_page = prev };
    }
    // SAFETY: `raw_page` is exclusively owned; clearing its links and state is unaliased.
    unsafe {
        (*raw_page.as_ptr()).next_page = None;
        (*raw_page.as_ptr()).prev_page = None;
        (*raw_page.as_ptr()).list_state = 0;
    }
    // SAFETY: `page_index` is initialized metadata in the exclusively-owned page.
    let page_index = unsafe { (*raw_page.as_ptr()).page_index };
    if page_index > 0 {
        // SAFETY: parent segment accessible via segment-mapping provenance.
        let segment = unsafe { Page::parent_segment_of(raw_page.as_ptr()) };
        // SAFETY: `page_linked_mask` is exclusively accessible via `segment`.
        unsafe { (*segment).page_linked_mask &= !(1 << page_index) };
    }
}

/// Non-generic core for atomic move between two intrusive page lists.
///
/// # Safety
///
/// `raw_page` must be exclusively owned and currently linked in `from_head_slot`.
/// Every page reachable from both lists must be exclusively accessible.
#[inline(always)]
pub(crate) unsafe fn move_page_raw(
    raw_page: NonNull<Page>,
    from_head_slot: &mut Option<NonNull<Page>>,
    to_head_slot: &mut Option<NonNull<Page>>,
    new_state: u8,
) {
    // SAFETY: `raw_page` is exclusively owned per the function's contract.
    let next = unsafe { (*raw_page.as_ptr()).next_page };
    let prev = unsafe { (*raw_page.as_ptr()).prev_page };

    if let Some(prev_ptr) = prev {
        // SAFETY: caller's contract covers every page in both lists.
        unsafe { (*prev_ptr.as_ptr()).next_page = next };
    } else {
        *from_head_slot = next;
    }
    if let Some(next_ptr) = next {
        // SAFETY: same exclusivity contract as the prev-pointer write.
        unsafe { (*next_ptr.as_ptr()).prev_page = prev };
    }

    let head = *to_head_slot;
    // SAFETY: `raw_page` is exclusively owned; link writes are unaliased.
    unsafe {
        (*raw_page.as_ptr()).next_page = head;
        (*raw_page.as_ptr()).prev_page = None;
    }
    if let Some(head_ptr) = head {
        // SAFETY: caller's contract covers the destination head page.
        unsafe { (*head_ptr.as_ptr()).prev_page = Some(raw_page) };
    }
    *to_head_slot = Some(raw_page);
    // SAFETY: `raw_page` is exclusively owned; `list_state` write is unaliased.
    unsafe { (*raw_page.as_ptr()).list_state = new_state };
}
