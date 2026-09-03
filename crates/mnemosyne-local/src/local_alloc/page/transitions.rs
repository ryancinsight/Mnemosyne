use crate::local_alloc::ThreadAllocator;
use core::ptr::NonNull;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use mnemosyne_core::types::Page;

use super::lists::{
    PageListToken, move_page_between_lists_branded, push_page_front, unlink_page_from_list,
    with_page_list_token,
};

#[inline(always)]
unsafe fn unlink_empty_page_with_token<'id, B: HasSegmentPool>(
    token: &mut PageListToken<'id, B>,
    head_slot: &mut Option<NonNull<Page>>,
    target: NonNull<Page>,
) -> bool {
    // SAFETY: the caller guarantees `target` is a valid page owned by this
    // allocator; reading `list_state` is a plain field load.
    if unsafe { target.as_ref() }.list_state == 3 {
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
            // SAFETY: `page_ptr` names a live page owned by this allocator, and
            // `token` is the matching list-permission witness for branding it.
            let page = unsafe { token.page(page_ptr) };
            // SAFETY: `class` indexes this allocator's `active_pages` array and
            // `page` is branded by the same token, so linking it at the front
            // preserves the intrusive-list ownership invariant.
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
            // SAFETY: `page_ptr` names a live page owned by this allocator, and
            // `token` is the matching list-permission witness for branding it.
            let page = unsafe { token.page(page_ptr) };
            // SAFETY: `class` indexes this allocator's `full_pages` array and
            // `page` is branded by the same token, so linking it at the front
            // preserves the intrusive-list ownership invariant.
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
            // SAFETY: `page_ptr` names a live page owned by this allocator, and
            // `token` is the matching list-permission witness for branding it.
            let page = unsafe { token.page(page_ptr) };
            // SAFETY: `page` is branded by the same token that guards
            // `self.empty_pages`, so pushing it to that intrusive list is sound.
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
        // SAFETY: `target` is non-null (checked above) and the caller
        // guarantees it points to a valid page owned by this allocator.
        if unsafe { target.as_ref() }.list_state == 2 {
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
        // SAFETY: the caller guarantees `page_ptr` points to a valid page
        // owned by this allocator; reading `list_state` is a plain field load.
        if unsafe { page_ptr.as_ref() }.list_state != 2 {
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
        // SAFETY: `target` is non-null (checked above) and the caller
        // guarantees it points to a valid page owned by this allocator.
        let page = unsafe { target.as_ref() };
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
        // SAFETY: `target` is non-null (checked above) and the caller
        // guarantees it points to a valid page owned by this allocator.
        if unsafe { target.as_ref() }.list_state == 3 {
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
                // SAFETY: every list node is a live page projected from its
                // complete segment mapping by the allocator's routing path.
                let segment = unsafe { Page::parent_segment_of(page_ptr.as_ptr()) };

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
