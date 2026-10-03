//! A block being freed, reached through the two pointers that own its bytes.
//!
//! `dealloc` receives the caller's pointer, whose provenance may cover only
//! the requested bytes (a `Box<i32>` covers 4 bytes of a 16-byte block), and
//! std may still protect it (`ThreadInit::init(self: Box<Self>)` frees its own
//! box). Under Tree Borrows a write to protected bytes must come through a
//! child of the caller's tag; under Stacked Borrows a write past the caller's
//! bytes must not. [`FreedBlock`] therefore writes and reads byte `i` of the
//! block through the caller's pointer when `i < covered`, and through the
//! mapping-derived block pointer otherwise (ADR 0012, option 1). Free lists
//! store only the mapping-derived pointer.

use super::{Block, Segment};
use crate::constants::{PAGE_SHIFT, PAGES_PER_SEGMENT, SEGMENT_SIZE};
use core::ptr::NonNull;

/// The block a free is returning, with the caller's covered prefix.
#[derive(Clone, Copy)]
pub struct FreedBlock {
    block: NonNull<Block>,
    caller: *mut u8,
    covered: usize,
}

impl FreedBlock {
    /// Pairs the mapping-derived `block` with the caller's pointer.
    ///
    /// `covered` is the number of leading bytes the caller's provenance covers:
    /// the request size on the Rust `dealloc` path, `usize::MAX` where no layout
    /// is known (C `free`, layout-free entry points), meaning the whole block.
    ///
    /// # Safety
    ///
    /// `block` must carry its mapping's provenance and `caller` the freeing
    /// caller's; both must address the same live block, and the caller's
    /// provenance must cover `min(covered, block size)` bytes from it.
    #[inline(always)]
    #[must_use]
    pub unsafe fn new(block: NonNull<Block>, caller: *mut u8, covered: usize) -> Self {
        debug_assert_eq!(
            block.as_ptr().addr(),
            caller.addr(),
            "invariant: one block, two provenances"
        );
        Self {
            block,
            caller,
            covered,
        }
    }

    /// The mapping-derived block pointer, the only form a free list may store.
    #[inline(always)]
    #[must_use]
    pub fn block(self) -> NonNull<Block> {
        self.block
    }

    /// The block's segment header and page index.
    ///
    /// Masks the mapping-derived block pointer, whose provenance covers the
    /// whole mapping and so the header, instead of consulting the registry
    /// again.
    #[inline(always)]
    #[must_use]
    pub fn segment(self) -> (*mut Segment, usize) {
        let block = self.block.as_ptr().cast::<u8>();
        let segment = block
            .map_addr(|addr| addr & !(SEGMENT_SIZE - 1))
            .cast::<Segment>();
        (
            segment,
            (block.addr() >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1),
        )
    }

    /// Writes `len` bytes from `src` at `offset`, splitting at `covered`.
    ///
    /// # Safety
    ///
    /// `offset + len` must lie inside the block and `src` must be readable for
    /// `len` bytes.
    #[inline(always)]
    unsafe fn write_at(self, offset: usize, src: *const u8, len: usize) {
        let caller_len = self.covered.saturating_sub(offset).min(len);
        // SAFETY: `[offset, offset + caller_len)` lies below `covered`, inside
        // the caller's provenance; the remainder lies inside the block and the
        // mapping's provenance (`new`'s contract).
        unsafe {
            core::ptr::copy_nonoverlapping(src, self.caller.add(offset), caller_len);
            core::ptr::copy_nonoverlapping(
                src.add(caller_len),
                self.block.as_ptr().cast::<u8>().add(offset + caller_len),
                len - caller_len,
            );
        }
    }

    /// Reads `len` bytes at `offset` into `dst`, splitting at `covered`.
    ///
    /// # Safety
    ///
    /// As [`Self::write_at`], with `dst` writable for `len` bytes.
    #[inline(always)]
    unsafe fn read_at(self, offset: usize, dst: *mut u8, len: usize) {
        let caller_len = self.covered.saturating_sub(offset).min(len);
        // SAFETY: as in `write_at`.
        unsafe {
            core::ptr::copy_nonoverlapping(self.caller.add(offset), dst, caller_len);
            core::ptr::copy_nonoverlapping(
                self.block.as_ptr().cast::<u8>().add(offset + caller_len),
                dst.add(caller_len),
                len - caller_len,
            );
        }
    }

    /// Writes the free-list link, encoding it when `encrypted`.
    ///
    /// # Safety
    ///
    /// `encrypted` and `page_cookie` must come from the owning segment header.
    #[inline(always)]
    pub unsafe fn set_next_dynamic(
        self,
        next: Option<NonNull<Block>>,
        encrypted: bool,
        page_cookie: usize,
    ) {
        let link = if encrypted {
            Block::encode_link(next, page_cookie)
        } else {
            next
        };
        if self.covered >= size_of::<Block>() {
            // SAFETY: the caller's provenance covers the whole aligned link.
            unsafe { self.caller.cast::<Option<NonNull<Block>>>().write(link) };
        } else {
            // SAFETY: the link lies at offset 0 inside the block; the byte copy
            // carries the pointer's provenance byte by byte.
            unsafe { self.write_at(0, (&raw const link).cast(), size_of::<Block>()) };
        }
    }

    /// Fills `min(covered, len)` bytes from the start with `byte`.
    ///
    /// Only the caller's bytes held user data; slack past the request never
    /// did, so poisoning stops at the request.
    ///
    /// # Safety
    ///
    /// `len` must not exceed the block.
    #[inline(always)]
    pub unsafe fn poison(self, byte: u8, len: usize) {
        // SAFETY: `min(covered, len)` bytes lie inside the caller's provenance.
        unsafe { core::ptr::write_bytes(self.caller, byte, self.covered.min(len)) };
    }

    /// Writes the backward-edge canary word at `block + size_of::<Block>()`.
    ///
    /// # Safety
    ///
    /// The block must be at least two words long; `page_cookie` must be the
    /// owning page's cookie.
    #[inline(always)]
    pub unsafe fn write_free_canary(self, page_cookie: usize) {
        let canary = Block::canary_value(self.block.as_ptr(), page_cookie);
        // SAFETY: the canary slot is the block's second word.
        unsafe {
            self.write_at(
                size_of::<Block>(),
                (&raw const canary).cast(),
                size_of::<usize>(),
            )
        };
    }

    /// Whether the canary slot holds this block's canary (a double free).
    ///
    /// # Safety
    ///
    /// As [`Self::write_free_canary`].
    #[inline(always)]
    #[must_use]
    pub unsafe fn has_free_canary(self, page_cookie: usize) -> bool {
        let mut word = 0_usize;
        // SAFETY: as in `write_free_canary`.
        unsafe {
            self.read_at(
                size_of::<Block>(),
                (&raw mut word).cast(),
                size_of::<usize>(),
            )
        };
        // `0` is the cleared-slot sentinel `Block::clear_free_canary` writes.
        word != 0 && word == Block::canary_value(self.block.as_ptr(), page_cookie)
    }
}
