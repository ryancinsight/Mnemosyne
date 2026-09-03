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

    /// Computes the address-bound free canary value for a block.
    ///
    /// `canary = FREE_CANARY_MAGIC ^ page_cookie ^ (block_addr >> 4)`
    ///
    /// Binding the canary to the block's own address prevents a valid canary
    /// from being copied from one block to another within the same page.
    #[inline(always)]
    fn canary_value(block: *const Block, page_cookie: usize) -> usize {
        FREE_CANARY_MAGIC ^ page_cookie ^ (block.addr() >> 4)
    }

    /// Writes the backward-edge free canary at `block + size_of::<Block>()`.
    ///
    /// Called by the free path under `HardenedPolicy` (`ENABLE_FREE_LIST_ENCRYPTION`)
    /// to mark the block as on the free list. The canary is address-bound:
    /// `FREE_CANARY_MAGIC ^ page_cookie ^ (block_addr >> 4)`.
    ///
    /// # Safety
    ///
    /// `block` must point to a live block whose allocation is at least
    /// `2 * size_of::<Block>()` bytes; the canary slot at
    /// `block + size_of::<Block>()` must lie within the allocation.
    #[inline(always)]
    pub unsafe fn write_free_canary(block: *mut Block, page_cookie: usize) {
        // SAFETY: the canary slot is at `block + size_of::<Block>()`, which is
        // within the block's allocation by the caller's contract
        // (minimum block size >= 2 * size_of::<Block>()).
        unsafe {
            block
                .cast::<usize>()
                .add(1)
                .write(Self::canary_value(block, page_cookie));
        }
    }

    /// Returns `true` if the backward-edge canary is present, indicating a
    /// likely double-free. Called before writing the new free-list link so
    /// double-frees are caught before corrupting the list.
    ///
    /// # Safety
    ///
    /// Same requirements as [`write_free_canary`](Block::write_free_canary).
    #[inline(always)]
    pub unsafe fn check_double_free(block: *const Block, page_cookie: usize) -> bool {
        // SAFETY: the canary slot is within the block by the caller's contract.
        let observed = unsafe { block.cast::<usize>().add(1).read() };
        observed == Self::canary_value(block, page_cookie)
    }

    /// Clears the backward-edge free canary when a block is taken off the
    /// free list, preventing a stale canary from triggering a false positive
    /// on the next free.
    ///
    /// # Safety
    ///
    /// Same requirements as [`write_free_canary`](Block::write_free_canary).
    #[inline(always)]
    pub unsafe fn clear_free_canary(block: *mut Block) {
        // SAFETY: the canary slot is within the block by the caller's contract.
        unsafe { block.cast::<usize>().add(1).write(0) };
    }
}

/// Magic value mixed into every free-list backward-edge canary.
/// Non-zero to distinguish a canary from a zeroed (uninitialized) slot.
pub const FREE_CANARY_MAGIC: usize = 0xDEAD_C0DE_CAFE_BABE_usize;

// Block is `#[repr(transparent)]` over one pointer, so it is exactly one
// pointer wide. The canary sits at `block + size_of::<Block>()` and must stay
// within the minimum block size (MIN_BLOCK_SIZE = 16 bytes = 2 pointers on
// 64-bit). This assertion enforces the invariant at compile time.
const _: () = assert!(
    core::mem::size_of::<Block>() == core::mem::size_of::<*mut u8>(),
    "Block must be exactly one pointer wide for the canary slot to be in bounds"
);

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
