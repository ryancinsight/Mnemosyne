//! Backend mappings paired with their chunk-registry entries (ADR 0012).
//!
//! Every mapping the arena takes from `B` is registered in
//! [`registry`](mnemosyne_core::types::segment::registry) before any block in
//! it is handed out, and unregistered before it is released, so the free path
//! can always rebuild metadata pointers from the mapping's own provenance.

use mnemosyne_backend::DefaultBackend;
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
    match unsafe { register(mapping, len) } {
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
    if !released && unsafe { register(mapping, len) }.is_err() {
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

/// Registers `mapping[..len]`, taking registry leaves from the host's
/// [`DefaultBackend`].
///
/// Leaves outlive every mapping they index, so they never come from the
/// mapping's own backend: a device backend's memory (CUDA host-pinned or
/// managed) dies with its context, which would leave a dangling leaf in the
/// process-wide root.
///
/// # Safety
///
/// As [`registry::register_mapping`] for `mapping`/`len`.
unsafe fn register(mapping: *mut u8, len: usize) -> Result<(), RegistrationError> {
    // SAFETY: leaves are fresh host mappings of `LEAF_BYTES` that the host
    // never reclaims on its own, so an installed leaf stays live for the
    // process; a leaf the registry does not install goes straight back. A
    // refused release of that leaf leaves it mapped and unused, which is a
    // leak of `LEAF_BYTES`, not a fault.
    unsafe {
        registry::register_mapping(
            mapping,
            len,
            || DefaultBackend::allocate(LEAF_BYTES),
            |leaf| {
                let _released = DefaultBackend::deallocate(leaf, LEAF_BYTES);
            },
        )
    }
}

#[cfg(all(test, target_pointer_width = "64"))]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use mnemosyne_core::constants::SEGMENT_SIZE;

    /// Base of a 16 GiB registry-leaf span far above the mappings a host hands
    /// out, so no other mapping has installed its leaf.
    const FABRICATED: usize = (1 << 45) + 7 * (1 << 34);

    static ALLOCATE_CALLS: AtomicUsize = AtomicUsize::new(0);

    /// Hands out the fabricated mapping address, counting calls. The registry
    /// stores a mapping pointer without dereferencing it, so no memory lies
    /// behind the address.
    struct FabricatedBackend;

    impl MemoryBackend for FabricatedBackend {
        unsafe fn allocate(_size: usize) -> *mut u8 {
            ALLOCATE_CALLS.fetch_add(1, Ordering::Relaxed);
            core::ptr::without_provenance_mut(FABRICATED)
        }

        unsafe fn deallocate(_ptr: *mut u8, _size: usize) -> bool {
            true
        }
    }

    #[test]
    fn registry_leaves_never_come_from_the_mapping_backend() {
        // SAFETY: nothing dereferences the fabricated mapping; `unmap` below
        // releases it.
        let mapping = unsafe { map::<FabricatedBackend>(SEGMENT_SIZE) };
        assert_eq!(mapping.addr(), FABRICATED);
        assert_eq!(
            ALLOCATE_CALLS.load(Ordering::Relaxed),
            1,
            "the registry leaf for a fresh span was taken from the mapping's backend"
        );
        assert_eq!(registry::mapping_donor(FABRICATED + 1).addr(), FABRICATED);

        // SAFETY: `mapping` came from `map` with this length; nothing in it
        // is freed afterwards.
        assert!(unsafe { unmap::<FabricatedBackend>(mapping, SEGMENT_SIZE) });
        assert!(registry::mapping_donor(FABRICATED + 1).is_null());
    }
}
