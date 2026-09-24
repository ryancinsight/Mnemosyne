use mnemosyne_core::policy::AllocPolicy;

/// Non-generic SSOT for byte-initialization at allocation time.
///
/// Called `P::ZERO_INITIALIZE`/`P::ENABLE_POISONING`/`P::POISON_ALLOC_BYTE` booleans are
/// const-propagated at every call site, so all branches that don't apply are
/// dead-code-eliminated with zero runtime cost.
///
/// # Safety
///
/// `ptr` must be valid for writes of `size` bytes.
#[inline(always)]
pub(crate) unsafe fn init_bytes(
    ptr: *mut u8,
    size: usize,
    zero_init: bool,
    poison: bool,
    poison_byte: u8,
) {
    if zero_init {
        // SAFETY: `ptr` valid for `size` bytes per contract.
        unsafe { core::ptr::write_bytes(ptr, 0, size) };
    } else if poison {
        // SAFETY: `ptr` valid for `size` bytes per contract.
        unsafe { core::ptr::write_bytes(ptr, poison_byte, size) };
    }
}

/// Non-generic SSOT for byte-poisoning at deallocation time.
///
/// # Safety
///
/// `ptr` must be valid for writes of `size` bytes until the surrounding free
/// operation completes.
#[inline(always)]
pub(crate) unsafe fn poison_bytes(ptr: *mut u8, size: usize, poison: bool, poison_byte: u8) {
    if poison {
        // SAFETY: `ptr` valid for `size` bytes per contract.
        unsafe { core::ptr::write_bytes(ptr, poison_byte, size) };
    }
}

/// Applies allocation-time initialization required by `P`.
///
/// # Safety
///
/// `ptr` must be valid for writes of `size` bytes and must refer to memory
/// owned by the current allocation operation.
#[inline(always)]
pub unsafe fn initialize_allocated_bytes<P: AllocPolicy>(ptr: *mut u8, size: usize) {
    // SAFETY: forwarded — same contract.
    unsafe {
        init_bytes(
            ptr,
            size,
            P::ZERO_INITIALIZE,
            P::ENABLE_POISONING,
            P::POISON_ALLOC_BYTE,
        )
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
    // SAFETY: forwarded — same contract.
    unsafe { poison_bytes(ptr, size, P::ENABLE_POISONING, P::POISON_FREE_BYTE) }
}
