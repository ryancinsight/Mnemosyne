//! Free-block list node: the intrusive link written into a freed block.

use crate::abort::abort_on_corruption;
use core::ptr::NonNull;

/// A node representing a free block.
///
/// Free blocks are stored inline within the allocated memory when free.
#[repr(transparent)]
pub struct Block {
    /// Encrypted or raw pointer to the next free block.
    next_encoded: Option<NonNull<Block>>,
}

impl Block {
    /// Gets the next block in the free list, decoding it if required.
    ///
    /// # Safety
    ///
    /// The block pointer must be valid and aligned.
    #[inline(always)]
    pub unsafe fn get_next<P: crate::policy::AllocPolicy>(
        &self,
        page_cookie: usize,
    ) -> Option<NonNull<Block>> {
        // The const `P::ENABLE_FREE_LIST_ENCRYPTION` const-propagates into the
        // `encrypted` branch of `get_next_dynamic`, so the concrete codegen is
        // identical to a hand-inlined const form while the XOR-decode body and
        // its SAFETY argument live in one place.
        // SAFETY: forwarded unchanged from this method's `# Safety` contract —
        // the block pointer is valid and aligned.
        unsafe { self.get_next_dynamic(P::ENABLE_FREE_LIST_ENCRYPTION, page_cookie) }
    }

    /// Gets the next block dynamically using a dynamic encrypted flag.
    ///
    /// # Safety
    ///
    /// The block pointer must be valid and aligned.
    #[inline(always)]
    pub unsafe fn get_next_dynamic(
        &self,
        encrypted: bool,
        page_cookie: usize,
    ) -> Option<NonNull<Block>> {
        if encrypted {
            // SAFETY: the caller passed the owning segment's recorded mode and
            // its cookie together, which is exactly `get_next_raw_decoded`'s
            // contract -- the cookie decodes what that mode encoded.
            unsafe { self.get_next_raw_decoded(page_cookie) }
        } else {
            // SAFETY: the caller must have already validated that the owning
            // segment is in raw free-list mode; otherwise this silently reads an
            // encoded next pointer and bypasses the cookie that protects the
            // free-list metadata.
            unsafe { self.get_next_raw() }
        }
    }

    /// Returns the raw, unencoded next pointer without doing the encrypted
    /// branch or cookie work.
    ///
    /// This is intentionally unsafe: callers must have already proven the
    /// owning segment is in the raw free-list mode, otherwise this bypasses the
    /// XOR-encoded metadata protection used by the encrypted policy.
    #[inline(always)]
    pub(crate) unsafe fn get_next_raw(&self) -> Option<NonNull<Block>> {
        self.next_encoded
    }

    /// Decodes the XOR-obfuscated next pointer under the segment cookie.
    ///
    /// # Safety
    /// `page_cookie` must be the cookie the link was encoded with -- the
    /// owning segment's `keys[page_index]` for the page this block belongs
    /// to. Decoding under any other cookie yields a wild pointer that is
    /// still non-null and still looks like a `Block`, so a wrong cookie is
    /// not detectable here and corrupts the chain at its first use.
    #[inline(always)]
    pub unsafe fn get_next_raw_decoded(&self, page_cookie: usize) -> Option<NonNull<Block>> {
        self.next_encoded.map(|encoded| {
            let cookie = page_cookie | 1;
            let decoded_ptr = encoded.as_ptr().map_addr(|addr| addr ^ cookie);
            // SAFETY: same argument as `get_next` — the odd `cookie` flips
            // the low bit of the even, aligned original address, so the
            // decoded pointer is necessarily non-null.
            unsafe { NonNull::new_unchecked(decoded_ptr) }
        })
    }

    /// Sets the next block in the free list, encoding it if required.
    ///
    /// # Safety
    ///
    /// The block pointer must be valid and aligned.
    #[inline(always)]
    pub unsafe fn set_next<P: crate::policy::AllocPolicy>(
        &mut self,
        next: Option<NonNull<Block>>,
        page_cookie: usize,
    ) {
        // The const `P::ENABLE_FREE_LIST_ENCRYPTION` const-propagates into the
        // `encrypted` branch of `set_next_dynamic`, keeping the XOR-encode body
        // and its SAFETY argument in one place at identical codegen.
        // SAFETY: forwarded unchanged from this method's `# Safety` contract —
        // the block pointer is valid and aligned.
        unsafe { self.set_next_dynamic(next, P::ENABLE_FREE_LIST_ENCRYPTION, page_cookie) }
    }

    /// Sets the next block dynamically using a dynamic encrypted flag.
    ///
    /// # Safety
    ///
    /// The block pointer must be valid and aligned.
    #[inline(always)]
    pub unsafe fn set_next_dynamic(
        &mut self,
        next: Option<NonNull<Block>>,
        encrypted: bool,
        page_cookie: usize,
    ) {
        if encrypted {
            // SAFETY: same pairing as the decode above -- the mode and the
            // cookie come from one segment header, so the link is encoded with
            // the key its owner will decode it with.
            unsafe {
                self.set_next_raw_encoded(next, page_cookie);
            }
        } else {
            // SAFETY: the caller must have already validated that the owning
            // segment is in raw free-list mode; otherwise this writes an encoded
            // free-list link as if it were raw and bypasses the envelope that
            // keeps the encrypted metadata intact.
            unsafe { self.set_next_raw(next) }
        }
    }

    /// Writes the raw, unencoded next pointer without the encrypted branch.
    ///
    /// This is intentionally unsafe: callers must have already proven the
    /// owning segment is in the raw free-list mode, otherwise they can overwrite
    /// the encrypted metadata with an unencoded link and violate the mode
    /// invariant.
    #[inline(always)]
    pub(crate) unsafe fn set_next_raw(&mut self, next: Option<NonNull<Block>>) {
        self.next_encoded = next;
    }

    /// XOR-encodes the next pointer with the segment cookie for hardened mode.
    ///
    /// # Safety
    /// `page_cookie` must be the owning segment's `keys[page_index]` for this
    /// block's page, and `next` must point into the same page's free list.
    /// The link is only recoverable by a decode under that same cookie (ADR
    /// 0001), so encoding under a foreign one publishes an undecodable link.
    #[inline(always)]
    pub unsafe fn set_next_raw_encoded(
        &mut self,
        next: Option<NonNull<Block>>,
        page_cookie: usize,
    ) {
        self.next_encoded = Self::encode_link(next, page_cookie);
    }

    /// Encodes a free-list link with `page_cookie`, as stored in the block.
    #[inline(always)]
    pub(crate) fn encode_link(
        next: Option<NonNull<Block>>,
        page_cookie: usize,
    ) -> Option<NonNull<Block>> {
        next.map(|ptr| {
            let cookie = page_cookie | 1;
            let encoded_ptr = ptr.as_ptr().map_addr(|addr| addr ^ cookie);
            // SAFETY: `ptr` is non-null and aligned, the odd `cookie` flips its
            // low bit, so the encoded address is non-null.
            unsafe { NonNull::new_unchecked(encoded_ptr) }
        })
    }
}

// SAFETY: `Block` is a `#[repr(transparent)]` free-list node holding a single
// optional next-link that lives inline in the block's own memory only while the
// block is free. It carries no thread-affine state (no `Cell`, no thread id, no
// `Rc`), and every cross-thread access is serialized by the allocator's
// ownership protocol: a free block belongs to exactly one page's free list at a
// time, and cross-thread frees are published through that page's
// `AtomicFreeList` (acquire/release), which establishes the happens-before edge
// guarding the link. Transferring ownership of a `Block` between threads is
// therefore sound.
unsafe impl Send for Block {}
// SAFETY: shared `&Block` access across threads never races because the inline
// next-link is mutated only by the single thread that owns the containing page,
// with the `AtomicFreeList` publish/consume serializing any hand-off; the type
// exposes no other interior mutability.
unsafe impl Sync for Block {}

// ── Backward-edge free canary ─────────────────────────────────────────────────
//
// A canary word written at `block + size_of::<Block>()` (the second pointer
// slot) detects double-frees under `HardenedPolicy`.
//
// The canary uses a multiplicative formula inspired by snmalloc 0.7.x
// `freelist.h::signed_prev`:
//
//   canary = (addr + MAGIC).wrapping_mul(cookie ^ (addr >> 4))
//
// This is stronger than XOR-only because the multiplier is non-linear: knowing
// `MAGIC` and `addr` is not enough to forge the value without `cookie`.

/// Magic constant mixed into the backward-edge canary.
///
/// Written as a `u64` and narrowed: a `usize` literal this wide does not
/// compile on the 32-bit targets this crate still supports (see the
/// `cfg(not(target_pointer_width = "64"))` arms in `crate::sync`). Truncation
/// is the intent — the canary is a bit-mixing constant, not a quantity — and
/// the 64-bit value is unchanged, with the low half serving 32-bit builds.
pub const FREE_CANARY_MAGIC: usize = 0xDEAD_C0DE_CAFE_BABE_u64 as usize;

// `Block` is pointer-wide; the canary sits at `block + size_of::<Block>()`.
// Both slots must fit in `MIN_BLOCK_SIZE` (16 bytes = 2 × 8-byte pointers on
// 64-bit). Enforced at build time:
const _: () = assert!(
    core::mem::size_of::<Block>() * 2 <= crate::constants::MIN_BLOCK_SIZE,
    "canary slot does not fit within MIN_BLOCK_SIZE"
);

impl Block {
    #[inline(always)]
    fn validate_canary_slot(block: *const Block) {
        if block.is_null() {
            abort_on_corruption("free canary block pointer is null");
        }
        let addr = block.addr();
        if addr < core::mem::align_of::<usize>() {
            abort_on_corruption(
                "free canary block pointer is stale or below the minimum alignment",
            );
        }
        if !addr.is_multiple_of(core::mem::align_of::<usize>()) {
            abort_on_corruption("free canary block pointer is misaligned");
        }
    }

    /// Returns a raw pointer to the canary slot (the second `usize` in the
    /// block) without validation. Callers must have already called
    /// `validate_canary_slot`.
    #[inline(always)]
    fn canary_slot_ptr(block: *mut Block) -> *mut usize {
        // SAFETY: callers guarantee block is valid and the block is at least
        // 2 × size_of::<Block>() bytes so the adjacent slot is in bounds.
        unsafe { block.cast::<usize>().add(1) }
    }

    /// Computes the multiplicative backward-edge canary value for `block`.
    #[inline(always)]
    pub(crate) fn canary_value(block: *const Block, page_cookie: usize) -> usize {
        Self::validate_canary_slot(block);
        let addr = block.addr();
        addr.wrapping_add(FREE_CANARY_MAGIC)
            .wrapping_mul(page_cookie ^ (addr >> 4))
    }

    /// Clears the canary when a block is taken off the free list.
    ///
    /// # Safety
    ///
    /// Same requirements as
    /// [`FreedBlock::write_free_canary`](crate::types::FreedBlock::write_free_canary).
    #[inline(always)]
    pub unsafe fn clear_free_canary(block: *mut Block) {
        Self::validate_canary_slot(block);
        // SAFETY: the canary slot is within the block by the caller's contract.
        unsafe { Self::canary_slot_ptr(block).write(0) };
    }
}
