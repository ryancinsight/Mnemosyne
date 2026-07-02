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
                let decoded_ptr = (encoded.as_ptr() as usize ^ cookie) as *mut Block;
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
                let encoded_ptr = (ptr.as_ptr() as usize ^ cookie) as *mut Block;
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
