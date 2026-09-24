use crate::local_alloc::ThreadAllocator;
use core::marker::PhantomData;
use core::ptr::NonNull;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::types::Page;

type PageListBrand<'id, B> = fn(&'id mut ThreadAllocator<B>) -> &'id mut ThreadAllocator<B>;

/// Zero-sized permission proving exclusive allocator authority over page-list
/// metadata for one mutation step.
pub(crate) struct PageListToken<'id, B: HasSegmentPool> {
    _brand: PhantomData<PageListBrand<'id, B>>,
}

impl<'id, B: HasSegmentPool> PageListToken<'id, B> {
    #[inline(always)]
    fn new() -> Self {
        Self {
            _brand: PhantomData,
        }
    }

    /// Brands `page_ptr` with this allocator-list permission.
    ///
    /// # Safety
    ///
    /// `page_ptr` must identify a live page whose list metadata is owned by
    /// the allocator used to construct this token.
    #[inline(always)]
    pub(crate) unsafe fn page(&mut self, page_ptr: NonNull<Page>) -> BrandedPage<'id> {
        BrandedPage {
            ptr: page_ptr,
            _brand: PhantomData,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct BrandedPage<'id> {
    ptr: NonNull<Page>,
    _brand: PhantomData<fn(&'id mut Page) -> &'id mut Page>,
}

impl BrandedPage<'_> {
    #[inline(always)]
    fn ptr(self) -> NonNull<Page> {
        self.ptr
    }
}

#[inline(always)]
pub(crate) fn with_page_list_token<B: HasSegmentPool, R>(
    f: impl for<'id> FnOnce(PageListToken<'id, B>) -> R,
) -> R {
    f(PageListToken::new())
}

/// Pushes `page_ptr` to the front of a branded intrusive page list.
///
/// The type-safety contract (exclusive page-list access) is enforced by
/// `token`; the heavy logic lives in the non-generic [`push_page_front_raw`]
/// helper that compiles once regardless of the backend `B`.
///
/// # Safety
///
/// `page_ptr` and every page currently linked from `head_slot` must belong to
/// the allocator-list permission represented by `token`.
#[inline(always)]
pub(crate) unsafe fn push_page_front<'id, B: HasSegmentPool>(
    _token: &mut PageListToken<'id, B>,
    head_slot: &mut Option<NonNull<Page>>,
    page_ptr: BrandedPage<'id>,
    list_state: u8,
) {
    // SAFETY: the caller's `token` contract guarantees exclusive access to
    // `page_ptr` and every page reachable from `head_slot`.
    unsafe { push_page_front_raw(page_ptr.ptr(), head_slot, list_state) }
}

/// Unlinks the page identified by `page_ptr` from the doubly-linked list
/// whose head is stored in `head_slot`.
///
/// This operation is O(1) and mutates at most three pointer fields.
///
/// # Safety
///
/// `page_ptr` must be branded by the same allocator-list permission as every
/// page reachable from `head_slot`, and must be currently linked in that list.
/// Unlinks `page_ptr` from the doubly-linked branded intrusive list.
///
/// The type-safety contract is enforced by `token`; the heavy logic lives in
/// the non-generic [`unlink_page_from_list_raw`] helper that compiles once.
///
/// # Safety
///
/// `page_ptr` must be branded by the same allocator-list permission as every
/// page reachable from `head_slot`, and must be currently linked in that list.
#[inline(always)]
pub(crate) unsafe fn unlink_page_from_list<'id, B: HasSegmentPool>(
    _token: &mut PageListToken<'id, B>,
    head_slot: &mut Option<NonNull<Page>>,
    page_ptr: BrandedPage<'id>,
) {
    // SAFETY: the caller's `token` contract guarantees exclusive access to
    // `page_ptr` and every page reachable from `head_slot`.
    unsafe { unlink_page_from_list_raw(page_ptr.ptr(), head_slot) }
}

/// Moves `page_ptr` from one branded list to the front of another in one pass.
///
/// See the module-level doc for the rationale on not touching `page_linked_mask`.
/// The type-safety contract is enforced by `token`; the heavy logic lives in
/// the non-generic [`move_page_raw`] helper that compiles once.
///
/// # Safety
///
/// `page_ptr` must be branded and currently linked in the `from_head_slot` list,
/// and every page reachable from either list must belong to `token`.
#[inline(always)]
pub(crate) unsafe fn move_page_between_lists_branded<'id, B: HasSegmentPool>(
    _token: &mut PageListToken<'id, B>,
    from_head_slot: &mut Option<NonNull<Page>>,
    to_head_slot: &mut Option<NonNull<Page>>,
    page_ptr: BrandedPage<'id>,
    new_state: u8,
) {
    // SAFETY: the caller's `token` contract guarantees exclusive access to
    // `page_ptr` and every page reachable from both lists.
    unsafe { move_page_raw(page_ptr.ptr(), from_head_slot, to_head_slot, new_state) }
}

// ── Non-generic raw implementations ─────────────────────────────────────────
//
// `PageListToken<'id, B>` is a ZST (zero runtime bytes) and every
// `token.page(ptr).ptr()` call reduces to the identity function on `NonNull<Page>`.
// Extracting the page-list logic here means it compiles **once** instead of
// once per backend `B`, while the thin branded facades above preserve the
// compile-time ownership contract.

/// Non-generic core of [`push_page_front`].
///
/// # Safety
///
/// `raw_page` must be exclusively owned by the calling allocator's page-list
/// authority and must not already be linked into `head_slot`.
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

/// Non-generic core of [`unlink_page_from_list`].
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

/// Non-generic core of [`move_page_between_lists_branded`].
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
