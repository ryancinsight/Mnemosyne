//! Address-arithmetic helpers: recovering a `(Segment, page_index)` pair or
//! a page-metadata pointer from a user allocation pointer.
//!
//! These two free functions are the **single authoritative**
//! pointer→(segment, page\_index) and pointer→page classifiers shared by the
//! free, realloc, and usable-size fast paths.

use super::{Segment, registry};
use crate::constants::{PAGE_SHIFT, PAGES_PER_SEGMENT, SEGMENT_SIZE};
use crate::types::Page;

/// Recovers the parent segment header and page index for a user pointer.
///
/// Every small allocation lives inside a `SEGMENT_ALIGN`-aligned segment, so
/// masking the address down to `SEGMENT_SIZE` yields the segment header, and
/// the mid-address `PAGE_SHIFT` bits (masked by `PAGES_PER_SEGMENT - 1`) yield
/// the page index. This is the single authoritative pointer→(segment,
/// page_index) classifier shared by the free, realloc, and usable-size paths.
///
/// Only the address of `ptr` is used. The header pointer takes its provenance
/// from the [`registry`](super::registry) entry of the mapping that holds the
/// chunk, never from `ptr`: a caller's pointer may carry provenance covering
/// only its own bytes (ADR 0012), and ADR 0009 rules out exposed provenance.
///
/// # Safety
///
/// `ptr` must be a non-null pointer returned by a Mnemosyne small/huge
/// allocation, so the recovered segment header is live and the page index is a
/// valid index into its `pages` array. An address no registered mapping covers
/// aborts as corruption.
#[inline(always)]
pub unsafe fn locate_segment(ptr: *mut u8) -> (*mut Segment, usize) {
    let addr = ptr.addr();
    let segment = mapping_pointer(addr, addr & !(SEGMENT_SIZE - 1)).cast::<Segment>();
    let page_index = (addr >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    (segment, page_index)
}

/// Rebuilds `ptr` with its mapping's provenance.
///
/// An allocation handed back to a caller (an in-place `realloc` result) must
/// cover its whole block, not only the bytes the caller's previous pointer
/// covered.
///
/// # Safety
///
/// `ptr` must lie inside a live Mnemosyne small or huge allocation.
#[inline(always)]
#[must_use]
pub unsafe fn locate_block(ptr: *mut u8) -> *mut u8 {
    let addr = ptr.addr();
    mapping_pointer(addr, addr)
}

/// Reads the huge-allocation back-pointer stored one slot before `ptr`.
///
/// The slot lies outside the caller's bytes, so it is read through the
/// registered mapping's provenance rather than `ptr.sub(1)`.
///
/// # Safety
///
/// `ptr` must be the start of a live large/huge allocation, whose metadata slot
/// precedes it inside the same mapping.
#[inline(always)]
#[must_use]
pub unsafe fn huge_back_pointer(ptr: *mut u8) -> *mut Segment {
    let addr = ptr.addr();
    let slot = mapping_pointer(addr, addr - size_of::<*mut Segment>()).cast::<*mut Segment>();
    // SAFETY: the slot is inside the live mapping registered for `ptr`'s chunk
    // (the caller's contract) and `slot` carries that mapping's provenance.
    unsafe { slot.read() }
}

/// A pointer to `target` carrying the provenance of the mapping that the
/// registry records for the chunk containing `key`.
#[inline(always)]
fn mapping_pointer(key: usize, target: usize) -> *mut u8 {
    let donor = registry::mapping_donor(key);
    if donor.is_null() {
        crate::abort::abort_on_corruption("pointer is not inside a registered Mnemosyne mapping");
    }
    donor.with_addr(target)
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
