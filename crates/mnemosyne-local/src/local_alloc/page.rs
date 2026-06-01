use crate::local_alloc::ThreadAllocator;
use core::ptr::NonNull;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
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

/// Unlinks the page identified by `page_ptr` from the doubly-linked list
/// whose head is stored in `head_slot`.
///
/// This operation is O(1) and mutates at most three pointer fields.
///
/// # Safety
///
/// `page_ptr` must be a valid, live page pointer owned by the current thread
/// and must be currently linked in the list starting at `head_slot`.
#[inline(always)]
pub(crate) unsafe fn unlink_page_from_list(
    head_slot: &mut Option<NonNull<Page>>,
    mut page_ptr: NonNull<Page>,
) {
    let page = page_ptr.as_mut();
    let next = page.next_page;
    let prev = page.prev_page;

    if let Some(mut prev_ptr) = prev {
        prev_ptr.as_mut().next_page = next;
    } else {
        *head_slot = next;
    }

    if let Some(mut next_ptr) = next {
        next_ptr.as_mut().prev_page = prev;
    }

    page.next_page = None;
    page.prev_page = None;
    page.list_state = 0;
}

/// Reclaims any pending cross-thread frees on `page` and, if reclamation
/// added blocks to the local free list, pops one block and increments the
/// page's `alloc_count`.
///
/// Returns the popped block when reclamation succeeded, or `None` when
/// `page.thread_free` was empty.
///
/// # Safety
///
/// Same contract as `Page::reclaim_thread_free`: the page must belong to
/// the allocator context performing the reconciliation and every block in
/// `page.thread_free` must belong to this page.
#[inline(always)]
pub(crate) unsafe fn try_reclaim_and_allocate<P: AllocPolicy>(
    page: &mut Page,
) -> Option<NonNull<Block>> {
    let reclaimed = unsafe { page.reclaim_thread_free::<P>() };
    if reclaimed == 0 {
        return None;
    }
    super::record_cross_thread_reclaimed(reclaimed);
    // Safety: `reclaim_thread_free` returning a nonzero count guarantees
    // that the drained chain is now linked onto `page.free`.
    let block = unsafe { pop_page_free_block::<P>(page) };
    page.alloc_count += 1;
    Some(block)
}

impl<B: HasSegmentPool> ThreadAllocator<B> {
    #[inline(always)]
    pub(crate) unsafe fn push_active_page(&mut self, page_ptr: NonNull<Page>, class: usize) {
        let page = &mut *page_ptr.as_ptr();
        page.next_page = *self.active_pages.get_unchecked(class);
        page.prev_page = None;
        if let Some(mut head) = *self.active_pages.get_unchecked(class) {
            head.as_mut().prev_page = Some(page_ptr);
        }
        *self.active_pages.get_unchecked_mut(class) = Some(page_ptr);
        page.list_state = 1;
    }

    #[inline(always)]
    pub(crate) unsafe fn push_full_page(&mut self, page_ptr: NonNull<Page>, class: usize) {
        let page = &mut *page_ptr.as_ptr();
        page.next_page = *self.full_pages.get_unchecked(class);
        page.prev_page = None;
        if let Some(mut head) = *self.full_pages.get_unchecked(class) {
            head.as_mut().prev_page = Some(page_ptr);
        }
        *self.full_pages.get_unchecked_mut(class) = Some(page_ptr);
        page.list_state = 2;
    }

    #[inline(always)]
    pub(crate) unsafe fn push_empty_page(&mut self, page_ptr: NonNull<Page>) {
        let page = &mut *page_ptr.as_ptr();
        page.next_page = self.empty_pages;
        page.prev_page = None;
        if let Some(mut head) = self.empty_pages {
            head.as_mut().prev_page = Some(page_ptr);
        }
        self.empty_pages = Some(page_ptr);
        page.list_state = 3;
    }

    /// Helper to unlink a page specifically from the full pages list of a class.
    #[inline]
    #[must_use]
    pub(crate) unsafe fn unlink_full_page(&mut self, page_ptr: *mut Page, class: usize) -> bool {
        debug_assert!(class < NUM_SIZE_CLASSES);
        let Some(target) = NonNull::new(page_ptr) else {
            return false;
        };
        if target.as_ref().list_state == 2 {
            unlink_page_from_list(self.full_pages.get_unchecked_mut(class), target);
            true
        } else {
            false
        }
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
        if page.list_state == 1 {
            unlink_page_from_list(self.active_pages.get_unchecked_mut(class), target);
        } else if page.list_state == 2 {
            unlink_page_from_list(self.full_pages.get_unchecked_mut(class), target);
        }
    }

    /// Helper to unlink a page from the empty pages list.
    #[inline]
    pub(crate) unsafe fn unlink_empty_page(&mut self, page_ptr: *mut Page) -> bool {
        let Some(target) = NonNull::new(page_ptr) else {
            return false;
        };
        if target.as_ref().list_state == 3 {
            unlink_page_from_list(&mut self.empty_pages, target);
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

        let mut curr = self.empty_pages;
        while let Some(page_ptr) = curr {
            let page_addr = page_ptr.as_ptr() as usize;
            let segment_addr = page_addr & !(SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;

            // Check if there are other active allocations in this segment using the occupancy bitmask.
            let has_other_allocations = unsafe { (*segment).page_occupied_mask != 0 };

            if has_other_allocations {
                // Found an empty page in a dirty segment! Unlink and return it.
                unsafe {
                    self.unlink_empty_page(page_ptr.as_ptr());
                }
                return Some(page_ptr);
            }

            curr = unsafe { page_ptr.as_ref().next_page };
        }

        // Fall back to LIFO (the head of the empty_pages list)
        if let Some(page_ptr) = self.empty_pages {
            unsafe {
                self.unlink_empty_page(page_ptr.as_ptr());
            }
            Some(page_ptr)
        } else {
            None
        }
    }
}
