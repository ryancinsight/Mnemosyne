use crate::local_alloc::ThreadAllocator;
use core::marker::PhantomData;
use core::ptr::NonNull;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Block, Page};

type PageListBrand<'id, B> = fn(&'id mut ThreadAllocator<B>) -> &'id mut ThreadAllocator<B>;

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
    let mut raw_page = page_ptr.ptr();
    let page = unsafe { raw_page.as_mut() };
    page.next_page = *head_slot;
    page.prev_page = None;
    if let Some(mut head) = *head_slot {
        // Safety: the caller's token contract covers every page linked from
        // `head_slot`.
        let _head = unsafe { token.page(head) };
        head.as_mut().prev_page = Some(raw_page);
    }
    *head_slot = Some(raw_page);
    page.list_state = list_state;
    if page.page_index > 0 {
        let segment = page.parent_segment();
        unsafe {
            (*segment).page_linked_mask |= 1 << page.page_index;
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
    let mut raw_page = page_ptr.ptr();
    let page = unsafe { raw_page.as_mut() };
    let next = page.next_page;
    let prev = page.prev_page;

    if let Some(mut prev_ptr) = prev {
        // Safety: the caller's token contract covers adjacent pages in the
        // same intrusive list.
        let _prev = unsafe { token.page(prev_ptr) };
        prev_ptr.as_mut().next_page = next;
    } else {
        *head_slot = next;
    }

    if let Some(mut next_ptr) = next {
        // Safety: the caller's token contract covers adjacent pages in the
        // same intrusive list.
        let _next = unsafe { token.page(next_ptr) };
        next_ptr.as_mut().prev_page = prev;
    }

    page.next_page = None;
    page.prev_page = None;
    page.list_state = 0;
    if page.page_index > 0 {
        let segment = page.parent_segment();
        unsafe {
            (*segment).page_linked_mask &= !(1 << page.page_index);
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
    let mut raw_page = page_ptr.ptr();
    let page = unsafe { raw_page.as_mut() };
    let next = page.next_page;
    let prev = page.prev_page;

    // Unlink page from the source list.
    if let Some(mut prev_ptr) = prev {
        let _prev = unsafe { token.page(prev_ptr) };
        prev_ptr.as_mut().next_page = next;
    } else {
        *from_head_slot = next;
    }

    if let Some(mut next_ptr) = next {
        let _next = unsafe { token.page(next_ptr) };
        next_ptr.as_mut().prev_page = prev;
    }

    // Push page to the front of the destination list.
    let head = *to_head_slot;
    page.next_page = head;
    page.prev_page = None;
    if let Some(mut head_ptr) = head {
        let _head = unsafe { token.page(head_ptr) };
        head_ptr.as_mut().prev_page = Some(raw_page);
    }
    *to_head_slot = Some(raw_page);
    page.list_state = new_state;
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

#[inline(always)]
unsafe fn unlink_empty_page_with_token<'id, B: HasSegmentPool>(
    token: &mut PageListToken<'id, B>,
    head_slot: &mut Option<NonNull<Page>>,
    target: NonNull<Page>,
) -> bool {
    if target.as_ref().list_state == 3 {
        let page = unsafe { token.page(target) };
        unsafe { unlink_page_from_list(token, head_slot, page) };
        true
    } else {
        false
    }
}

impl<B: HasSegmentPool> ThreadAllocator<B> {
    #[inline(always)]
    pub(crate) unsafe fn push_active_page(&mut self, page_ptr: NonNull<Page>, class: usize) {
        with_page_list_token::<B, _>(|mut token| {
            let page = unsafe { token.page(page_ptr) };
            unsafe {
                push_page_front(
                    &mut token,
                    self.active_pages.get_unchecked_mut(class),
                    page,
                    1,
                )
            };
        });
    }

    #[inline(always)]
    pub(crate) unsafe fn push_full_page(&mut self, page_ptr: NonNull<Page>, class: usize) {
        with_page_list_token::<B, _>(|mut token| {
            let page = unsafe { token.page(page_ptr) };
            unsafe {
                push_page_front(
                    &mut token,
                    self.full_pages.get_unchecked_mut(class),
                    page,
                    2,
                )
            };
        });
    }

    #[inline(always)]
    pub(crate) unsafe fn push_empty_page(&mut self, page_ptr: NonNull<Page>) {
        with_page_list_token::<B, _>(|mut token| {
            let page = unsafe { token.page(page_ptr) };
            unsafe { push_page_front(&mut token, &mut self.empty_pages, page, 3) };
        });
    }

    /// Helper to unlink a page specifically from the full pages list of a class.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) unsafe fn unlink_full_page(&mut self, page_ptr: *mut Page, class: usize) -> bool {
        debug_assert!(class < NUM_SIZE_CLASSES);
        let Some(target) = NonNull::new(page_ptr) else {
            return false;
        };
        if target.as_ref().list_state == 2 {
            with_page_list_token::<B, _>(|mut token| {
                let page = unsafe { token.page(target) };
                unsafe {
                    unlink_page_from_list(
                        &mut token,
                        self.full_pages.get_unchecked_mut(class),
                        page,
                    )
                };
            });
            true
        } else {
            false
        }
    }

    /// Moves a linked full page back to the active list for `class`.
    ///
    /// This is the same metadata transition as `unlink_full_page` followed by
    /// `push_active_page`, but it carries one page-list token through both
    /// operations. The caller must already have allocator-list authority.
    #[inline(always)]
    #[must_use]
    pub(crate) unsafe fn move_full_page_to_active(
        &mut self,
        page_ptr: NonNull<Page>,
        class: usize,
    ) -> bool {
        debug_assert!(class < NUM_SIZE_CLASSES);
        if page_ptr.as_ref().list_state != 2 {
            return false;
        }
        with_page_list_token::<B, _>(|mut token| {
            let page = unsafe { token.page(page_ptr) };
            unsafe {
                move_page_between_lists_branded(
                    &mut token,
                    self.full_pages.get_unchecked_mut(class),
                    self.active_pages.get_unchecked_mut(class),
                    page,
                    1,
                );
            }
        });
        true
    }

    /// Helper to unlink a page from the active pages or full pages list of a class.
    #[inline]
    pub(crate) unsafe fn unlink_page(&mut self, page_ptr: *mut Page, class: usize) {
        debug_assert!(class < NUM_SIZE_CLASSES);
        let Some(target) = NonNull::new(page_ptr) else {
            return;
        };
        let page = target.as_ref();
        debug_assert_eq!(page.size_class as usize, class);
        let list_state = page.list_state;
        with_page_list_token::<B, _>(|mut token| {
            let branded_page = unsafe { token.page(target) };
            if list_state == 1 {
                unsafe {
                    unlink_page_from_list(
                        &mut token,
                        self.active_pages.get_unchecked_mut(class),
                        branded_page,
                    )
                };
            } else if list_state == 2 {
                unsafe {
                    unlink_page_from_list(
                        &mut token,
                        self.full_pages.get_unchecked_mut(class),
                        branded_page,
                    )
                };
            }
        });
    }

    /// Helper to unlink a page from the empty pages list.
    #[inline]
    pub(crate) unsafe fn unlink_empty_page(&mut self, page_ptr: *mut Page) -> bool {
        let Some(target) = NonNull::new(page_ptr) else {
            return false;
        };
        if target.as_ref().list_state == 3 {
            with_page_list_token::<B, _>(|mut token| {
                unsafe { unlink_empty_page_with_token(&mut token, &mut self.empty_pages, target) };
            });
            true
        } else {
            false
        }
    }

    /// Pops the best empty page from the recycling list, prioritizing pages
    /// belonging to segments that are already dirty (contain other active pages).
    /// If no such page is found, falls back to the head of the empty page list (LIFO).
    pub(crate) unsafe fn pop_best_empty_page(&mut self) -> Option<NonNull<Page>> {
        use mnemosyne_core::constants::SEGMENT_SIZE;
        use mnemosyne_core::types::Segment;

        // Count each recycling sweep: the scan below walks the empty-page list
        // (bounded to 16) preferring a page whose segment already holds other
        // live allocations. Only count a sweep that has something to scan.
        if self.empty_pages.is_some() {
            self.recycle_sweeps += 1;
        }

        with_page_list_token::<B, _>(|mut token| {
            let mut curr = self.empty_pages;
            let mut checked = 0;
            while let Some(page_ptr) = curr {
                if checked >= 16 {
                    break;
                }
                checked += 1;
                let page_addr = page_ptr.as_ptr() as usize;
                let segment_addr = page_addr & !(SEGMENT_SIZE - 1);
                let segment = segment_addr as *mut Segment;

                // Check if there are other active allocations in this segment using the occupancy bitmask.
                let has_other_allocations = unsafe { (*segment).page_occupied_mask != 0 };

                if has_other_allocations {
                    // Found an empty page in a dirty segment! Unlink and return it.
                    unsafe {
                        unlink_empty_page_with_token(&mut token, &mut self.empty_pages, page_ptr);
                    }
                    return Some(page_ptr);
                }

                curr = unsafe { page_ptr.as_ref().next_page };
            }

            // Fall back to LIFO (the head of the empty_pages list)
            if let Some(page_ptr) = self.empty_pages {
                unsafe {
                    unlink_empty_page_with_token(&mut token, &mut self.empty_pages, page_ptr);
                }
                Some(page_ptr)
            } else {
                None
            }
        })
    }
}
