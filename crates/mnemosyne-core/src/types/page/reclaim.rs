//! Page cross-thread free-list reclamation.
//!
//! These `impl Page` methods atomically drain the page's `thread_free`
//! (cross-thread deallocation) queue back into the page-local free list,
//! validating the drained chain; split from the page type definition by
//! Separation of Concerns.

use crate::abort::abort_on_corruption;
use crate::types::{Page, Segment};

impl Page {
    /// Atomically drains cross-thread frees into the page-local free list dynamically.
    ///
    /// # Safety
    ///
    /// The page must belong to the allocator context currently reconciling its
    /// metadata.
    #[inline]
    pub unsafe fn reclaim_thread_free_dynamic(&mut self, encrypted: bool) -> usize {
        let segment = self.parent_segment();
        let page_index = self.index_in_segment();
        // SAFETY: `parent_segment` returns `self`'s parent header and
        // `page_index` is this page's index, satisfying the per-segment
        // preconditions of `reclaim_thread_free_dynamic_for_segment`.
        unsafe { self.reclaim_thread_free_dynamic_for_segment(encrypted, segment, page_index) }
    }

    /// Atomically drains cross-thread frees when the caller already knows the
    /// parent segment and page index.
    ///
    /// # Safety
    ///
    /// `segment` must be this page's parent segment, and `page_index` must be
    /// this page's index in `segment.pages`.
    #[inline]
    pub unsafe fn reclaim_thread_free_dynamic_for_segment(
        &mut self,
        encrypted: bool,
        segment: *mut Segment,
        page_index: usize,
    ) -> usize {
        debug_assert_eq!(
            self.page_index as usize, page_index,
            "segment-aware reclaim called with the wrong page index"
        );
        // SAFETY: caller's `# Safety` contract guarantees `segment` is the parent
        // header and `page_index` is this page's in-range index, satisfying
        // `cookie_for_dynamic`'s contract.
        let cookie = unsafe { (*segment).cookie_for_dynamic(encrypted, page_index) };

        let Some((block, count)) = self.thread_free.pop_all(encrypted, cookie) else {
            return 0;
        };

        if count > self.alloc_count {
            abort_on_corruption(
                "reclaimed cross-thread free count exceeds the page's live allocations",
            );
        }
        // SAFETY: `segment`/`page_index` are the caller-provided valid parent
        // header and in-range index; `count <= alloc_count` was just checked, so
        // the subtraction does not underflow.
        unsafe { self.set_alloc_count_for_segment(segment, page_index, self.alloc_count - count) };

        let page_start = (segment as usize) + (page_index << crate::constants::PAGE_SHIFT);
        let page_end = page_start + crate::constants::PAGE_SIZE;

        let mut last = block;
        let first_addr = last.as_ptr() as usize;
        if first_addr < page_start
            || first_addr + self.block_size > page_end
            || (first_addr & (crate::constants::MIN_BLOCK_SIZE - 1)) != 0
        {
            abort_on_corruption(
                "reclaimed cross-thread free chain head is outside its page or misaligned",
            );
        }

        let mut visited = 1;
        // SAFETY: `last` starts at the validated `block` head and each loop
        // iteration only advances to a `node` that is re-validated below to lie
        // within the page and be `MIN_BLOCK_SIZE`-aligned, so every
        // `last.as_ptr()` deref reads a valid, aligned `Block` taken from this
        // page's thread-free chain.
        while let Some(node) = unsafe { (*last.as_ptr()).get_next_dynamic(encrypted, cookie) } {
            visited += 1;
            if visited > count {
                abort_on_corruption(
                    "reclaimed cross-thread free chain is longer than its counted length",
                );
            }
            let node_addr = node.as_ptr() as usize;
            if node_addr < page_start
                || node_addr + self.block_size > page_end
                || (node_addr & (crate::constants::MIN_BLOCK_SIZE - 1)) != 0
            {
                abort_on_corruption(
                    "reclaimed cross-thread free node is outside its page or misaligned",
                );
            }
            last = node;
        }
        if visited != count {
            abort_on_corruption(
                "reclaimed cross-thread free chain is shorter than its counted length",
            );
        }

        if self.free.is_none() {
            self.free = Some(block);
        } else {
            // SAFETY: `last` is the validated tail node of the drained chain (in
            // bounds of the page and aligned), so writing its next-link to splice
            // the existing `self.free` list onto it is a valid, owner-exclusive
            // write of a `Block` this thread now owns.
            unsafe {
                (*last.as_ptr()).set_next_dynamic(self.free, encrypted, cookie);
            }
            self.free = Some(block);
        }
        count
    }

    /// Drains cross-thread frees only when the page-local queue is currently
    /// non-empty.
    ///
    /// This keeps sweep-style callers from issuing an atomic `pop_all` for
    /// pages that have no remote frees while preserving the same reclamation
    /// logic when the queue is populated.
    ///
    /// # Safety
    ///
    /// `segment` must be this page's parent segment, and `page_index` must be
    /// this page's index in `segment.pages`.
    #[inline]
    pub unsafe fn reclaim_thread_free_if_present_for_segment(
        &mut self,
        encrypted: bool,
        segment: *mut Segment,
        page_index: usize,
    ) -> usize {
        if self.thread_free.is_empty() {
            return 0;
        }
        // SAFETY: `segment`/`page_index` are forwarded unchanged from this
        // function's identical `# Safety` contract (valid parent header, in-range
        // page index), satisfying the callee's preconditions.
        unsafe { self.reclaim_thread_free_dynamic_for_segment(encrypted, segment, page_index) }
    }

    /// Atomically drains cross-thread frees into the page-local free list.
    ///
    /// # Safety
    ///
    /// The page must belong to the allocator context currently reconciling its
    /// metadata.
    #[inline]
    pub unsafe fn reclaim_thread_free<P: crate::policy::AllocPolicy>(&mut self) -> usize {
        // SAFETY: this function's `# Safety` contract — the page belongs to the
        // reconciling allocator context — is exactly the precondition of
        // `reclaim_thread_free_dynamic`, forwarded unchanged.
        unsafe { self.reclaim_thread_free_dynamic(P::ENABLE_FREE_LIST_ENCRYPTION) }
    }
}
