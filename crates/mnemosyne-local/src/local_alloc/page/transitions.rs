use crate::local_alloc::ThreadAllocator;
use core::ptr::NonNull;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::constants::NUM_SIZE_CLASSES;
use mnemosyne_core::types::Page;

use super::lists::{move_page_raw, push_page_front_raw, unlink_page_from_list_raw};

impl<B: HasSegmentPool> ThreadAllocator<B> {
    #[inline(always)]
    pub(crate) unsafe fn push_active_page(&mut self, page_ptr: NonNull<Page>, class: usize) {
        // SAFETY: `&mut self` proves exclusive access; `class < NUM_SIZE_CLASSES`
        // is validated by the caller.
        unsafe { push_page_front_raw(page_ptr, self.active_pages.get_unchecked_mut(class), 1) };
    }

    #[inline(always)]
    pub(crate) unsafe fn push_full_page(&mut self, page_ptr: NonNull<Page>, class: usize) {
        // SAFETY: same as `push_active_page`.
        unsafe { push_page_front_raw(page_ptr, self.full_pages.get_unchecked_mut(class), 2) };
    }

    #[inline(always)]
    pub(crate) unsafe fn push_empty_page(&mut self, page_ptr: NonNull<Page>) {
        // SAFETY: `&mut self` proves exclusive access to `empty_pages`.
        unsafe { push_page_front_raw(page_ptr, &mut self.empty_pages, 3) };
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
            // SAFETY: `&mut self` proves exclusive access; `class < NUM_SIZE_CLASSES`
            // validated above; `target` is currently linked in `full_pages[class]`.
            unsafe {
                unlink_page_from_list_raw(target, self.full_pages.get_unchecked_mut(class));
            }
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
        // SAFETY: `&mut self` proves exclusive access; page is currently linked
        // in `full_pages[class]` (list_state == 2, checked above).
        unsafe {
            move_page_raw(
                page_ptr,
                self.full_pages.get_unchecked_mut(class),
                self.active_pages.get_unchecked_mut(class),
                1,
            );
        }
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
        // SAFETY: `&mut self` proves exclusive access; `target` is linked in
        // the active or full list for `class` per the list_state check below.
        if list_state == 1 {
            unsafe {
                unlink_page_from_list_raw(target, self.active_pages.get_unchecked_mut(class));
            }
        } else if list_state == 2 {
            unsafe {
                unlink_page_from_list_raw(target, self.full_pages.get_unchecked_mut(class));
            }
        }
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
            // SAFETY: `&mut self` proves exclusive access; `target` is currently
            // linked in `empty_pages` (list_state == 3 confirmed above).
            unsafe { unlink_page_from_list_raw(target, &mut self.empty_pages) };
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

            let has_other_allocations = unsafe { (*segment).page_occupied_mask != 0 };

            if has_other_allocations {
                // Found an empty page in a dirty segment — unlink and return it.
                // SAFETY: `&mut self` exclusive; `page_ptr` linked in `empty_pages`.
                unsafe { unlink_page_from_list_raw(page_ptr, &mut self.empty_pages) };
                return Some(page_ptr);
            }

            curr = unsafe { page_ptr.as_ref().next_page };
        }

        // Fall back to LIFO head
        if let Some(page_ptr) = self.empty_pages {
            // SAFETY: `&mut self` exclusive; head is linked in `empty_pages`.
            unsafe { unlink_page_from_list_raw(page_ptr, &mut self.empty_pages) };
            Some(page_ptr)
        } else {
            None
        }
    }
}
