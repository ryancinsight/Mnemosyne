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
/// # Safety
///
/// `page_ptr` and every page currently linked from `head_slot` must belong to
/// the allocator-list permission represented by `token`.
#[inline(always)]
pub(crate) unsafe fn push_page_front<'id, B: HasSegmentPool>(
    token: &mut PageListToken<'id, B>,
    head_slot: &mut Option<NonNull<Page>>,
    page_ptr: BrandedPage<'id>,
    list_state: u8,
) {
    let raw_page = page_ptr.ptr();
    // SAFETY: `raw_page` is exclusively accessible via the branded token;
    // writing `next_page` and `prev_page` does not alias any other borrow.
    unsafe {
        (*raw_page.as_ptr()).next_page = *head_slot;
        (*raw_page.as_ptr()).prev_page = None;
    }
    if let Some(head) = *head_slot {
        // SAFETY: the caller's token contract covers every page linked from
        // `head_slot`, so the head pointer is valid and exclusively reachable
        // through this list walk.
        unsafe {
            let head = token.page(head).ptr();
            (*head.as_ptr()).prev_page = Some(raw_page);
        }
    }
    *head_slot = Some(raw_page);
    // SAFETY: `raw_page` is exclusively accessible via the branded token.
    unsafe { (*raw_page.as_ptr()).list_state = list_state };
    // SAFETY: `page_index` is initialized metadata; `raw_page` is exclusive.
    let page_index = unsafe { (*raw_page.as_ptr()).page_index };
    if page_index > 0 {
        // SAFETY: list nodes retain the complete segment mapping provenance
        // supplied when the allocator projected each page metadata header.
        let segment = unsafe { Page::parent_segment_of(raw_page.as_ptr()) };
        unsafe {
            (*segment).page_linked_mask |= 1 << page_index;
        }
    }
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
#[inline(always)]
pub(crate) unsafe fn unlink_page_from_list<'id, B: HasSegmentPool>(
    token: &mut PageListToken<'id, B>,
    head_slot: &mut Option<NonNull<Page>>,
    page_ptr: BrandedPage<'id>,
) {
    let raw_page = page_ptr.ptr();
    // SAFETY: `raw_page` is exclusively accessible via the branded token;
    // reading `next_page` and `prev_page` is sound.
    let next = unsafe { (*raw_page.as_ptr()).next_page };
    let prev = unsafe { (*raw_page.as_ptr()).prev_page };

    if let Some(prev_ptr) = prev {
        // SAFETY: the caller's token contract covers adjacent pages in the
        // same intrusive list, so `prev_ptr` is valid and exclusively
        // reachable through this list walk.
        unsafe {
            let prev_ptr = token.page(prev_ptr).ptr();
            (*prev_ptr.as_ptr()).next_page = next;
        }
    } else {
        *head_slot = next;
    }

    if let Some(next_ptr) = next {
        // SAFETY: the caller's token contract covers adjacent pages in the
        // same intrusive list, so `next_ptr` is valid and exclusively
        // reachable through this list walk.
        unsafe {
            let next_ptr = token.page(next_ptr).ptr();
            (*next_ptr.as_ptr()).prev_page = prev;
        }
    }

    // SAFETY: `raw_page` is exclusively accessible via the branded token;
    // clearing its list links and state is sound.
    unsafe {
        (*raw_page.as_ptr()).next_page = None;
        (*raw_page.as_ptr()).prev_page = None;
        (*raw_page.as_ptr()).list_state = 0;
    }
    // SAFETY: `page_index` is initialized metadata; `raw_page` is exclusive.
    let page_index = unsafe { (*raw_page.as_ptr()).page_index };
    if page_index > 0 {
        // SAFETY: as in `push_page_front`, this node is a raw projection from
        // its live parent segment mapping.
        let segment = unsafe { Page::parent_segment_of(raw_page.as_ptr()) };
        unsafe {
            (*segment).page_linked_mask &= !(1 << page_index);
        }
    }
}

/// Moves a page from the intrusive list rooted at `from_head_slot` to the front
/// of the list rooted at `to_head_slot`, in a single token pass, and stamps the
/// destination `new_state` (`1` = active, `3` = empty).
///
/// This is the one authoritative full→active / active→empty relink: the two
/// transitions differ only in their source slot, destination slot, and stored
/// `list_state`, so they share this body. Unlike separate
/// `unlink_page_from_list` + `push_page_front` calls, it does not touch
/// `page_linked_mask` (both source and destination are allocator page lists, so
/// the linked bit stays set throughout) — behavior identical to the previous
/// dedicated movers.
///
/// # Safety
///
/// `page_ptr` must be branded and currently linked in the `from_head_slot` list,
/// and every page reachable from either list must belong to `token`.
#[inline(always)]
pub(crate) unsafe fn move_page_between_lists_branded<'id, B: HasSegmentPool>(
    token: &mut PageListToken<'id, B>,
    from_head_slot: &mut Option<NonNull<Page>>,
    to_head_slot: &mut Option<NonNull<Page>>,
    page_ptr: BrandedPage<'id>,
    new_state: u8,
) {
    let raw_page = page_ptr.ptr();
    // SAFETY: `raw_page` is exclusively accessible via the branded token.
    let next = unsafe { (*raw_page.as_ptr()).next_page };
    let prev = unsafe { (*raw_page.as_ptr()).prev_page };

    // Unlink from source list.
    if let Some(prev_ptr) = prev {
        // SAFETY: the caller's token contract covers every page reachable from
        // either list, so `prev_ptr` is valid and exclusively reachable here.
        unsafe {
            let prev_ptr = token.page(prev_ptr).ptr();
            (*prev_ptr.as_ptr()).next_page = next;
        }
    } else {
        *from_head_slot = next;
    }

    if let Some(next_ptr) = next {
        // SAFETY: the caller's token contract covers every page reachable from
        // either list, so `next_ptr` is valid and exclusively reachable here.
        unsafe {
            let next_ptr = token.page(next_ptr).ptr();
            (*next_ptr.as_ptr()).prev_page = prev;
        }
    }

    // Push page to the front of the destination list.
    let head = *to_head_slot;
    // SAFETY: `raw_page` is exclusively accessible via the branded token;
    // inserting it at the head of the destination list is sound.
    unsafe {
        (*raw_page.as_ptr()).next_page = head;
        (*raw_page.as_ptr()).prev_page = None;
    }
    if let Some(head_ptr) = head {
        // SAFETY: the caller's token contract covers every page reachable from
        // either list, so `head_ptr` is valid and exclusively reachable here.
        unsafe {
            let head_ptr = token.page(head_ptr).ptr();
            (*head_ptr.as_ptr()).prev_page = Some(raw_page);
        }
    }
    *to_head_slot = Some(raw_page);
    // SAFETY: `raw_page` is exclusively accessible via the branded token.
    unsafe { (*raw_page.as_ptr()).list_state = new_state };
}
