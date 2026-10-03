//! Value tests for the registry's chunk arithmetic, address guard, rollback
//! and unregistration.
//!
//! Each test registers fabricated mapping addresses in its own leaf, so tests
//! sharing the process-wide [`ROOT`] never observe each other's donors. The
//! registry stores and returns `mapping` without dereferencing it, and no
//! test dereferences a donor, so no memory needs to lie behind those
//! addresses.

use super::*;
use std::alloc::{Layout, alloc, dealloc};

/// Base of the `k`-th test leaf, a quarter of the way up the address space,
/// clear of the low and top regions mappings come from. Leaf-aligned on 64-bit
/// targets; a 32-bit target has one leaf and uses only `k = 0`.
const fn test_leaf_base(k: usize) -> usize {
    ((1 << (CHUNK_BITS - 2)) + k * LEAF_LEN) * SEGMENT_SIZE
}

fn leaf_layout() -> Layout {
    Layout::from_size_align(LEAF_BYTES, align_of::<AtomicPtr<u8>>())
        .expect("invariant: LEAF_BYTES is a positive multiple of the pointer size")
}

/// Registers the fabricated mapping at `addr`, taking leaves from the global
/// allocator.
fn register_at(addr: usize, len: usize) -> Result<(), RegistrationError> {
    // SAFETY: the mapping is never dereferenced (module docs), so the liveness
    // the contract asks for, which serves the free path's `with_addr` rebuild,
    // is not relied on; leaves are global-allocator regions of `LEAF_BYTES`
    // that stay installed for the test process, and an uninstalled one is
    // freed with its own layout.
    unsafe {
        register_mapping(
            core::ptr::without_provenance_mut(addr),
            len,
            || alloc(leaf_layout()),
            |leaf| dealloc(leaf, leaf_layout()),
        )
    }
}

#[test]
fn chunk_range_counts_only_chunk_bases_inside_the_span() {
    let s = SEGMENT_SIZE;
    assert_eq!(chunk_range(s, s), 1..2);
    assert_eq!(chunk_range(s + 1, 2 * s), 2..4);
    assert_eq!(chunk_range(s, s + 1), 1..3);
    assert_eq!(chunk_range(1, s - 1), 1..1);
    assert_eq!(chunk_range(s - 1, 1), 1..1);
}

#[test]
fn a_span_without_a_chunk_base_is_refused() {
    let base = test_leaf_base(0);
    assert_eq!(
        register_at(base + 1, SEGMENT_SIZE - 1),
        Err(RegistrationError::NoChunkBase)
    );
    assert!(mapping_donor(base + 1).is_null());
}

#[test]
fn registered_chunks_resolve_to_the_mapping_until_unregistered() {
    let base = test_leaf_base(0) + 4 * SEGMENT_SIZE;
    let len = 3 * SEGMENT_SIZE;
    assert_eq!(register_at(base, len), Ok(()));
    for offset in [0, SEGMENT_SIZE + 7, len - 1] {
        assert_eq!(mapping_donor(base + offset).addr(), base, "offset {offset}");
    }
    assert!(mapping_donor(base - 1).is_null());
    assert!(mapping_donor(base + len).is_null());

    // SAFETY: `base`/`len` were registered above and nothing frees inside them.
    unsafe { unregister_mapping(core::ptr::without_provenance_mut(base), len) };
    for offset in [0, SEGMENT_SIZE + 7, len - 1] {
        assert!(mapping_donor(base + offset).is_null(), "offset {offset}");
    }
}

#[cfg(target_pointer_width = "64")]
#[test]
fn a_leaf_failure_unregisters_the_chunks_already_registered() {
    // Chunk `boundary - 1` lies in leaf 1, chunk `boundary` in leaf 2.
    let boundary = test_leaf_base(2);
    let base = boundary - SEGMENT_SIZE;
    let mut calls = 0;
    // SAFETY: as `register_at`; the second leaf request fails before any leaf
    // of this call is released, and the first leaf is installed.
    let result = unsafe {
        register_mapping(
            core::ptr::without_provenance_mut(base),
            2 * SEGMENT_SIZE,
            || {
                calls += 1;
                if calls == 1 {
                    alloc(leaf_layout())
                } else {
                    core::ptr::null_mut()
                }
            },
            |leaf| dealloc(leaf, leaf_layout()),
        )
    };
    assert_eq!(result, Err(RegistrationError::LeafUnavailable));
    assert_eq!(calls, 2);
    assert!(mapping_donor(base).is_null());
    assert!(mapping_donor(boundary).is_null());
}

#[cfg(target_pointer_width = "64")]
#[test]
fn mappings_ending_beyond_the_address_space_are_refused() {
    let top = 1usize << VA_BITS;
    assert_eq!(
        register_at(top, SEGMENT_SIZE),
        Err(RegistrationError::BeyondAddressSpace)
    );
    assert_eq!(
        register_at(top - SEGMENT_SIZE, 2 * SEGMENT_SIZE),
        Err(RegistrationError::BeyondAddressSpace)
    );
    assert_eq!(register_at(top - SEGMENT_SIZE, SEGMENT_SIZE), Ok(()));
    assert_eq!(mapping_donor(top - 1).addr(), top - SEGMENT_SIZE);
    // SAFETY: registered above; nothing frees inside it.
    unsafe {
        unregister_mapping(
            core::ptr::without_provenance_mut(top - SEGMENT_SIZE),
            SEGMENT_SIZE,
        )
    };
}
