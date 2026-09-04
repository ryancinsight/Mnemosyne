//! Free-block list node: the intrusive link written into a freed block.

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
            self.next_encoded.map(|encoded| {
                let cookie = page_cookie | 1;
                let decoded_ptr = encoded.as_ptr().map_addr(|addr| addr ^ cookie);
                // SAFETY: same argument as `get_next` — the odd `cookie` flips
                // the low bit of the even, aligned original address, so the
                // decoded pointer is necessarily non-null.
                unsafe { NonNull::new_unchecked(decoded_ptr) }
            })
        } else {
            self.next_encoded
        }
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
            self.next_encoded = next.map(|ptr| {
                let cookie = page_cookie | 1;
                let encoded_ptr = ptr.as_ptr().map_addr(|addr| addr ^ cookie);
                // SAFETY: same argument as `set_next` — `ptr` is non-null and
                // aligned, the odd `cookie` flips its low bit, so the encoded
                // address is non-null.
                unsafe { NonNull::new_unchecked(encoded_ptr) }
            });
        } else {
            self.next_encoded = next;
        }
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
    /// Computes the multiplicative backward-edge canary value for `block`.
    #[inline(always)]
    fn canary_value(block: *const Block, page_cookie: usize) -> usize {
        let addr = block.addr();
        addr.wrapping_add(FREE_CANARY_MAGIC)
            .wrapping_mul(page_cookie ^ (addr >> 4))
    }

    /// Writes the backward-edge canary at `block + size_of::<Block>()`.
    ///
    /// Called on the free path under `HardenedPolicy`.
    ///
    /// # Safety
    ///
    /// `block` must point to a live block whose allocation is at least
    /// `2 * size_of::<Block>()` bytes; the canary slot must lie within it.
    #[inline(always)]
    pub unsafe fn write_free_canary(block: *mut Block, page_cookie: usize) {
        // SAFETY: the canary slot is the second `usize` inside the block
        // (`block + 1` in pointer arithmetic). By the caller's contract the
        // block is at least 2 × size_of::<Block>() bytes, so the slot is
        // within the allocation and exclusively owned by the free path.
        unsafe {
            block
                .cast::<usize>()
                .add(1)
                .write(Self::canary_value(block, page_cookie));
        }
    }

    /// Returns `true` if the backward-edge canary is present (likely double-free).
    ///
    /// # Safety
    ///
    /// Same requirements as [`write_free_canary`][Block::write_free_canary].
    #[inline(always)]
    pub unsafe fn check_double_free(block: *const Block, page_cookie: usize) -> bool {
        // SAFETY: the canary slot is within the block by the caller's contract.
        let observed = unsafe { block.cast::<usize>().add(1).read() };
        observed == Self::canary_value(block, page_cookie)
    }

    /// Clears the canary when a block is taken off the free list.
    ///
    /// # Safety
    ///
    /// Same requirements as [`write_free_canary`][Block::write_free_canary].
    #[inline(always)]
    pub unsafe fn clear_free_canary(block: *mut Block) {
        // SAFETY: the canary slot is within the block by the caller's contract.
        unsafe { block.cast::<usize>().add(1).write(0) };
    }
}
