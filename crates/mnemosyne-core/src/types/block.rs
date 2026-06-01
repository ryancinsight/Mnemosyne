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
        if P::ENABLE_FREE_LIST_ENCRYPTION {
            self.next_encoded.map(|encoded| {
                let cookie = page_cookie | 1;
                let decoded_ptr = (encoded.as_ptr() as usize ^ cookie) as *mut Block;
                unsafe { NonNull::new_unchecked(decoded_ptr) }
            })
        } else {
            self.next_encoded
        }
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
        if P::ENABLE_FREE_LIST_ENCRYPTION {
            self.next_encoded = next.map(|ptr| {
                let cookie = page_cookie | 1;
                let encoded_ptr = (ptr.as_ptr() as usize ^ cookie) as *mut Block;
                unsafe { NonNull::new_unchecked(encoded_ptr) }
            });
        } else {
            self.next_encoded = next;
        }
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
                unsafe { NonNull::new_unchecked(encoded_ptr) }
            });
        } else {
            self.next_encoded = next;
        }
    }
}

// Block is simple data, safe to send/sync as a memory representation.
unsafe impl Send for Block {}
unsafe impl Sync for Block {}
