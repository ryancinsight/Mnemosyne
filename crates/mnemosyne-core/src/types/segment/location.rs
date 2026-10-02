//! Address-arithmetic helpers: recovering a `(Segment, page_index)` pair, an
//! allocator-rooted block pointer, a page-metadata pointer, or a large/huge
//! back-pointer slot from a user allocation pointer.
//!
//! This module is the one place allocator pointers are rebuilt from exposed
//! provenance (see Provenance below); everywhere else, ADR 0009's `map_addr`
//! derivation from the live mapping pointer applies.
//!
//! These free functions are the **single authoritative**
//! pointer→(segment, page\_index), pointer→block, pointer→page, and
//! pointer→back-pointer classifiers shared by the free, realloc, and
//! usable-size paths.
//!
//! # Provenance
//!
//! A pointer the allocator returned grants access to that allocation only. The
//! caller may narrow it further — `Box<[T]>` retags it `Unique` over exactly the
//! allocation's bytes — so a header reached by masking the *user* pointer is
//! outside the provenance it carries, and reading through it is undefined
//! behavior (Stacked Borrows: the tag is absent at the header; Tree Borrows: the
//! tag is disabled by the allocator's own metadata writes). Strict provenance
//! cannot express the recovery, since `with_addr`/`map_addr` keep exactly the
//! provenance at fault. Every mapping that holds allocator metadata therefore
//! has its base provenance exposed once, when it is mapped
//! ([`expose_mapping`]), and the classifiers below rebuild metadata pointers
//! from that exposed provenance with `with_exposed_provenance_mut`. Both
//! operations compile to nothing; they state the provenance contract the
//! abstract machine needs.

use super::Segment;
use crate::constants::{PAGE_SHIFT, PAGES_PER_SEGMENT, SEGMENT_SIZE};
use crate::types::Page;
use core::ptr::with_exposed_provenance_mut;

/// Exposes the provenance of a fresh OS mapping so the classifiers in this
/// module can recover metadata pointers inside it from bare addresses.
///
/// Called once per mapping, by the code that obtains it from the backend, before
/// any pointer into the mapping is handed to a caller. Exposure lasts for the
/// mapping's lifetime, so pooled and recycled segments need no second call.
#[inline(always)]
pub fn expose_mapping(mapping: *mut u8) {
    // Exposure is the effect; the address is already known to the caller.
    let _mapping_address: usize = mapping.expose_provenance();
}

/// Recovers the parent segment header and page index for a user pointer.
///
/// Every small allocation lives inside a `SEGMENT_ALIGN`-aligned segment, so
/// masking `ptr` down to `SEGMENT_SIZE` yields the segment header, and the
/// mid-address `PAGE_SHIFT` bits (masked by `PAGES_PER_SEGMENT - 1`) yield the
/// page index. This is the single authoritative pointer→(segment, page_index)
/// classifier shared by the free, realloc, and usable-size fast paths.
///
/// The returned segment pointer carries the mapping's exposed provenance, not
/// `ptr`'s (module docs), so it reaches the header whatever the caller did to
/// the pointer it was handed.
///
/// # Safety
///
/// `ptr` must be a non-null pointer returned by a Mnemosyne small/huge
/// allocation, so the recovered segment header is live and the page index is a
/// valid index into its `pages` array, and the mapping holding that segment must
/// have been passed to [`expose_mapping`].
#[inline(always)]
pub unsafe fn locate_segment(ptr: *mut u8) -> (*mut Segment, usize) {
    let ptr_val = ptr.addr();
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = with_exposed_provenance_mut::<Segment>(segment_addr);
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
    (segment, page_index)
}

/// Recovers an allocator-rooted pointer to the block at `ptr`'s address.
///
/// An in-place `realloc` returns the block it was handed as the new
/// allocation; this gives that result the mapping's provenance, so it covers
/// the grown bytes the caller's pointer may not (module docs). The per-CPU
/// cache, which holds the pointer a block was freed through and has no page
/// pointer to re-derive from, hands its blocks out through this too; a
/// free-list pop re-derives from its page instead (`Page::pop_block_dynamic`).
///
/// A freed block is *not* re-derived: the free path writes its poison and
/// free-list link through the caller's pointer. The caller lent that pointer
/// for exactly this allocation, and a write through any other pointer is
/// foreign to the caller's tag; under Tree Borrows that disables a tag the
/// caller still protects, as when std frees a `Box` argument inside the
/// function it was passed to.
///
/// # Safety
///
/// `ptr` must be a non-null pointer returned by a Mnemosyne small/huge
/// allocation whose mapping was passed to [`expose_mapping`].
#[inline(always)]
pub unsafe fn locate_block(ptr: *mut u8) -> *mut u8 {
    with_exposed_provenance_mut::<u8>(ptr.addr())
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

/// Recovers the back-pointer slot written immediately before a large/huge
/// payload, which holds the owning segment header.
///
/// The slot lies outside the user's allocation, so it is reached through the
/// mapping's exposed provenance (module docs), never through `ptr`'s.
///
/// # Safety
///
/// `ptr` must be a payload pointer returned by the large/huge allocation path,
/// whose mapping was passed to [`expose_mapping`], so the pointer-aligned slot
/// before it is live and initialized.
#[inline(always)]
pub unsafe fn locate_huge_back_pointer(ptr: *mut u8) -> *mut *mut Segment {
    let slot_addr = ptr.addr() - size_of::<*mut Segment>();
    with_exposed_provenance_mut::<*mut Segment>(slot_addr)
}
