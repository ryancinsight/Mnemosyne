use mnemosyne_core::policy::AllocPolicy;

/// Applies allocation-time initialization required by `P`.
///
/// # Safety
///
/// `ptr` must be valid for writes of `size` bytes and must refer to memory
/// owned by the current allocation operation.
#[inline(always)]
pub unsafe fn initialize_allocated_bytes<P: AllocPolicy>(ptr: *mut u8, size: usize) {
    if P::ZERO_INITIALIZE {
        // Safety: the caller guarantees ptr is valid and writable for size bytes.
        unsafe {
            core::ptr::write_bytes(ptr, 0, size);
        }
    } else if P::ENABLE_POISONING {
        // Safety: the caller guarantees ptr is valid and writable for size bytes.
        unsafe {
            core::ptr::write_bytes(ptr, P::POISON_ALLOC_BYTE, size);
        }
    }
}

/// Applies free-time poisoning required by `P`.
///
/// # Safety
///
/// `ptr` must be valid for writes of `size` bytes until the surrounding free
/// operation completes.
#[inline(always)]
pub unsafe fn poison_freed_bytes<P: AllocPolicy>(ptr: *mut u8, size: usize) {
    if P::ENABLE_POISONING {
        // Safety: the caller guarantees ptr is valid and writable for size bytes.
        unsafe {
            core::ptr::write_bytes(ptr, P::POISON_FREE_BYTE, size);
        }
    }
}
