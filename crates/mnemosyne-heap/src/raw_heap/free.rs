//! Deallocation path for [`RawHeap`]: small-page free-list return, the
//! re-entrancy-gated `free_owned`, and the large/huge release helpers.

use super::RawHeap;
use core::ptr::NonNull;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::LocalAllocatorSelector;
use mnemosyne_local::internal::{
    Block, HasSegmentPool, Segment, do_local_free_internal, ensure_options_initialized,
    free_large_or_huge_raw, poison_freed_bytes,
};

impl<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> RawHeap<P, B> {
    /// # Safety
    ///
    /// `ptr` must be null or a live block previously returned by this
    /// `RawHeap`'s `alloc`, not yet freed. The allocator must be
    /// exclusively accessible (no concurrent access; brand-confined to one
    /// thread).
    #[inline(always)]
    pub(crate) unsafe fn free_owned_unchecked(&self, ptr: *mut u8) {
        ensure_options_initialized();
        if ptr.is_null() {
            return;
        }

        // SAFETY: `ptr` was returned by this allocator, so it points into a
        // live segment mapping; `locate_segment` derives the header pointer by
        // `map_addr`, keeping the mapping's provenance rather than synthesizing
        // it from an integer (MN-456), and returns the mask-bounded page index.
        let (segment, page_index) = unsafe { mnemosyne_core::types::locate_segment(ptr) };
        // SAFETY: `segment` is that live header and `page_index` is bounded by
        // the `(PAGES_PER_SEGMENT - 1)` mask, so it indexes the `pages` array.
        // A raw place projection is used rather than `&mut Page`: the page's
        // own metadata is also written through segment-rooted projections
        // during the free below (`decrement_alloc_count_in_segment`), and a
        // live `&mut Page` across those writes is the aliasing violation
        // MN-438/MN-443 removed elsewhere.
        let page = unsafe { mnemosyne_core::types::locate_page(segment, page_index) };

        if mnemosyne_prof::is_active() {
            // SAFETY: `ptr`, `page`, and `page_index` are the live triple just
            // derived from a valid allocation; `allocation_size` only reads
            // metadata inside the originating segment/mapping.
            let size = unsafe { allocation_size(ptr, page, page_index) };
            mnemosyne_prof::on_free(ptr, size);
        }

        // SAFETY: `page` is the live page projection recovered above; its
        // `block_size` is initialized for every allocated block.
        if page_index == 0 || unsafe { (*page).block_size } == 0 {
            // SAFETY: `page_index == 0` or a zero `block_size` identifies a
            // large/huge allocation, whose deallocation path `ptr` was routed
            // through at alloc time; the `# Safety` contract guarantees `ptr`
            // is live and owned.
            unsafe { free_large_or_huge_raw::<B>(ptr, P::ENABLE_POISONING, P::POISON_FREE_BYTE) };
            return;
        }

        if P::ENABLE_POISONING {
            // SAFETY: small-page free — `(*page).block_size` is the exact block
            // stride of `page`, and `ptr` is a live block of that page, so
            // poisoning `block_size` bytes stays within the block.
            unsafe { poison_freed_bytes::<P>(ptr, (*page).block_size as usize) };
        }

        // SAFETY: `ptr` is a small-page block (`page_index != 0`,
        // `block_size != 0`) of the recovered `page`/`segment` at
        // `page_index`; `free_owned` consumes the matching block/page/segment
        // triple under the heap's exclusive access.
        unsafe { self.free_owned(ptr as *mut Block, page, segment, page_index) };
    }

    /// # Safety
    ///
    /// `block` must be a live block of `page`, which must be `page_index` of
    /// the live `segment`; the three must be the matching triple recovered
    /// from one allocation. The allocator must be exclusively accessible.
    #[inline(always)]
    unsafe fn free_owned(
        &self,
        block: *mut Block,
        page: *mut mnemosyne_core::types::Page,
        segment: *mut Segment,
        page_index: usize,
    ) {
        // SAFETY: `segment` is the live segment from the caller's matching
        // triple per this function's `# Safety` contract; `free_list_encrypted`
        // only reads the segment's own initialized policy flag.
        let encrypted = unsafe { mnemosyne_core::types::Segment::free_list_encrypted(segment) };
        // Gate before borrowing, as in `alloc_small`.
        if self.is_allocating.get() {
            // SAFETY: re-entrant free while the allocator is mid-operation;
            // `block` is a non-null live block of `page` (allocator
            // invariant), so `new_unchecked` is sound and the page-local
            // atomic free list takes ownership of it.
            unsafe {
                (*page)
                    .thread_free
                    .push_dynamic(NonNull::new_unchecked(block), encrypted);
            }
            return;
        }

        // SAFETY: `page` is the live page projection from the caller's
        // matching triple; these read its own initialized metadata.
        let page_free = unsafe { (*page).free };
        let page_alloc_count = unsafe { (*page).alloc_count };
        // SAFETY: `segment` is the live segment owning `page`; `page_index`
        // indexes its `keys` array (sized `PAGES_PER_SEGMENT`) and is the
        // page's own index, so the read is in bounds.
        let cookie = unsafe { Segment::cookie_for_dynamic(segment, encrypted, page_index) };
        // SAFETY: `segment` is the live header and this heap is its owner, which
        // is `Segment::is_current`'s owner-only contract.
        if page_alloc_count != 1 || unsafe { Segment::is_current(segment) } {
            // SAFETY: `block` is a live non-null block of `page` (allocator
            // invariant); writing its `next` link, publishing it as the
            // free-list head, and decrementing the page/segment occupancy all
            // stay within `page`/`segment` under exclusive access.
            // The occupancy decrement writes this same page's `alloc_count`
            // through a segment-rooted projection, so the free-list head is
            // published through the raw page pointer rather than a live
            // `&mut Page` held across it (MN-438/MN-443).
            unsafe {
                (*block).set_next_dynamic(page_free, encrypted, cookie);
                (*page).free = Some(NonNull::new_unchecked(block));
                mnemosyne_core::types::Page::decrement_alloc_count_in_segment(segment, page_index);
            }
            return;
        }

        self.is_allocating.set(true);
        // SAFETY: exclusive, thread-confined access per the `# Safety` contract
        // and the gate above.
        let alloc = unsafe { &mut *self.allocator.get() };
        // SAFETY: the matching `block`/`page`/`segment`/`page_index` triple
        // from the `# Safety` contract is passed to the internal free, which
        // runs under the exclusive `alloc` borrow with the `is_allocating`
        // guard set.
        let became_empty =
            unsafe { do_local_free_internal::<B>(alloc, block, page, segment, page_index) };

        if became_empty {
            // SAFETY: `alloc` is the exclusively-borrowed allocator; recording
            // a defrag operation only mutates its own bookkeeping.
            unsafe { alloc.record_defrag_operation(true) };
        }

        self.is_allocating.set(false);
    }
}

/// # Safety
///
/// # Safety
///
/// `ptr` must be a live block of this allocator and `page`/`page_index`
/// the segment page recovered from it, as in `free_owned_unchecked`.
#[inline(always)]
unsafe fn allocation_size(
    ptr: *mut u8,
    page: *const mnemosyne_core::types::Page,
    page_index: usize,
) -> usize {
    // SAFETY: the caller passes the live page projection matching `ptr`; the
    // raw form keeps this read off a whole-`Page` reference so it composes
    // with the segment-rooted writes on the free path.
    if page_index == 0 || unsafe { (*page).block_size } == 0 {
        // SAFETY: large/huge classification — the owning `*mut Segment` lives
        // in the slot directly preceding the user payload (written at alloc
        // time), so this read recovers the live segment pointer.
        let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        // SAFETY: `ptr`/`segment` are the live block and its owning segment;
        // `huge_or_large_size` reads only metadata inside that mapping.
        unsafe { huge_or_large_size(ptr, segment) }
    } else {
        // SAFETY: as above — `page` is the live projection for `ptr`.
        unsafe { (*page).block_size as usize }
    }
}

/// # Safety
///
/// `segment` must be the live segment owning `ptr`'s large/huge mapping
/// (as recovered from the metadata slot preceding the payload).
#[inline(always)]
unsafe fn huge_or_large_size(ptr: *mut u8, segment: *mut Segment) -> usize {
    // SAFETY: `segment` is the live owning segment; the actual reservation for a
    // large/huge block is the raw mapping suffix, which may exceed the
    // requested payload size when alignment or prefix slack is present.
    unsafe { (*segment).huge_mapping_suffix_from(ptr) }
}
