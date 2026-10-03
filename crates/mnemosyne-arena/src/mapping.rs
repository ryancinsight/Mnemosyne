//! Backend mappings paired with their chunk-registry entries (ADR 0012).
//!
//! Every mapping the arena takes from `B` is registered in
//! [`registry`](mnemosyne_core::types::segment::registry) before any block in
//! it is handed out, and unregistered before it is released, so the free path
//! can always rebuild metadata pointers from the mapping's own provenance.

use mnemosyne_core::MemoryBackend;
use mnemosyne_core::types::segment::registry::{self, LEAF_BYTES, RegistrationError};

/// Maps `len` bytes from `B` and registers them; null when either step fails.
///
/// # Safety
///
/// As `B::allocate`; the result must be released only through [`unmap`].
#[inline]
pub(crate) unsafe fn map<B: MemoryBackend>(len: usize) -> *mut u8 {
    // SAFETY: forwarded — the caller upholds `B::allocate`'s contract.
    let mapping = unsafe { B::allocate(len) };
    if mapping.is_null() {
        return mapping;
    }
    // SAFETY: `mapping` is the fresh, unregistered `len`-byte mapping above.
    match unsafe { register::<B>(mapping, len) } {
        Ok(()) => mapping,
        Err(_) => {
            // SAFETY: `mapping` came from `B::allocate(len)` and was never used.
            let _released = unsafe { B::deallocate(mapping, len) };
            core::ptr::null_mut()
        }
    }
}

/// Unregisters a mapping from [`map`] and releases it to `B`.
///
/// A release `B` refuses leaves the mapping live, so it is registered again
/// and stays reachable from the free path.
///
/// # Safety
///
/// `mapping`/`len` must come from [`map`], and no block inside the mapping may
/// be freed after a successful release.
#[inline]
pub(crate) unsafe fn unmap<B: MemoryBackend>(mapping: *mut u8, len: usize) -> bool {
    // SAFETY: `mapping` was registered by `map` with this `len`.
    unsafe { registry::unregister_mapping(mapping, len) };
    // SAFETY: forwarded — `mapping` came from `B::allocate(len)`.
    let released = unsafe { B::deallocate(mapping, len) };
    // SAFETY: the release failed, so `mapping` is still live and, after the
    // unregister above, unregistered.
    if !released && unsafe { register::<B>(mapping, len) }.is_err() {
        mnemosyne_core::abort::abort_on_corruption(
            "a retained mapping could not be re-registered after a refused release",
        );
    }
    released
}

/// Registers a test fixture's mapping that did not come from [`map`] (a stack
/// segment, a hand-built huge header), taking leaves from the global allocator.
///
/// # Safety
///
/// As [`registry::register_mapping`] for `mapping`/`len`.
#[cfg(test)]
pub(crate) unsafe fn register_fixture_mapping(mapping: *mut u8, len: usize) {
    let leaf_layout = std::alloc::Layout::from_size_align(LEAF_BYTES, align_of::<usize>())
        .expect("a leaf is a positive multiple of the pointer size");
    // SAFETY: leaves come from the global allocator and stay installed for the
    // test process; a leaf the registry does not install is freed at once.
    unsafe {
        registry::register_mapping(
            mapping,
            len,
            || std::alloc::alloc(leaf_layout),
            |leaf| std::alloc::dealloc(leaf, leaf_layout),
        )
    }
    .expect("a fixture mapping lies inside the registry's address range");
}

/// Registers `mapping[..len]`, taking registry leaves from `B`.
///
/// # Safety
///
/// As [`registry::register_mapping`] for `mapping`/`len`.
unsafe fn register<B: MemoryBackend>(
    mapping: *mut u8,
    len: usize,
) -> Result<(), RegistrationError> {
    // SAFETY: leaves are fresh `B` mappings of `LEAF_BYTES`, kept for the
    // process lifetime once installed; a leaf the registry does not install
    // goes straight back to `B`.
    unsafe {
        registry::register_mapping(
            mapping,
            len,
            || B::allocate(LEAF_BYTES),
            |leaf| {
                let _released = B::deallocate(leaf, LEAF_BYTES);
            },
        )
    }
}
