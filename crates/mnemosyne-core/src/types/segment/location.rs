//! Address-arithmetic helpers: recovering a `(Segment, page_index)` pair or
//! a page-metadata pointer from a user allocation pointer.
//!
//! These two free functions are the **single authoritative**
//! pointer→(segment, page\_index) and pointer→page classifiers shared by the
//! free, realloc, and usable-size fast paths.

use super::Segment;
use crate::constants::{PAGE_SHIFT, PAGES_PER_SEGMENT, SEGMENT_SIZE};
use crate::types::Page;

/// Recovers the parent segment header and page index for a user pointer.
///
/// Every small allocation lives inside a `SEGMENT_ALIGN`-aligned segment, so
/// masking `ptr` down to `SEGMENT_SIZE` yields the segment header, and the
/// mid-address `PAGE_SHIFT` bits (masked by `PAGES_PER_SEGMENT - 1`) yield the
/// page index. This is the single authoritative pointer→(segment, page_index)
/// classifier shared by the free, realloc, and usable-size fast paths.
///
/// # Safety
///
/// `ptr` must be a non-null pointer returned by a Mnemosyne small/huge
/// allocation, so the recovered segment header is live and the page index is a
/// valid index into its `pages` array.
#[inline(always)]
pub unsafe fn locate_segment(ptr: *mut u8) -> (*mut Segment, usize) {
    let ptr_val = ptr.addr();
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = ptr.map_addr(|_| segment_addr).cast::<Segment>();
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    (segment, page_index)
}

/// Recovers a page-metadata pointer without borrowing the enclosing mapping.
///
/// Mnemosyne stores allocator metadata and user blocks in one OS mapping.
/// Cached intrusive-list pointers therefore cross user alloc/free calls that
/// may invalidate reference-derived provenance. Metadata access must re-create
/// provenance from the live mapping address instead of retaining a tag from an
/// earlier `&mut Segment::pages` projection.
///
/// # Safety
///
/// `segment` must identify a live initialized segment and `page_index` must be
/// less than `PAGES_PER_SEGMENT`.
#[inline(always)]
pub unsafe fn locate_page(segment: *mut Segment, page_index: usize) -> *mut Page {
    debug_assert!(page_index < PAGES_PER_SEGMENT);
    // SAFETY: the caller guarantees a live segment and an in-range index. Keep
    // the page recovery on the canonical segment-derived path so every access
    // through the segment metadata shares one provenance and no stale borrow can
    // survive across the alloc/free or reclaim hops.
    unsafe { Page::page_in_segment(segment, page_index) }
}
