//! NUMA binding, interleave allocation, and first-touch tests.
//!
//! The Linux paths are exercised only when they are deterministic without
//! NUMA hardware: `mbind` on a freshly allocated (unfaulted) region to node
//! zero succeeds on any Linux system, and an out-of-range node id fails in
//! the wrapper before any kernel call. The tiered-heap Numa routing test
//! asserts the best-effort contract — a block is returned whether or not
//! the policy call succeeded — so it is portable across platforms.

use super::*;
#[cfg(target_os = "linux")]
use crate::numa::NumaError;
use crate::numa::{allocate_interleaved, bind_to_node, first_touch};
use crate::tier::{MemoryTier, PlacementHint, tier_for};
use crate::tiered_heap::scope_tiered;
use core::alloc::Layout;
use mnemosyne_core::StandardPolicy;
use themis::NumaNodeId;

#[test]
fn allocate_interleaved_returns_usable_aligned_memory() {
    let layout = Layout::from_size_align(64 * 1024, 4096).expect("valid layout");
    let ptr = allocate_interleaved(layout).expect("interleaved allocation succeeds");
    assert!(
        (ptr.as_ptr() as usize).is_multiple_of(4096),
        "page-aligned base"
    );
    // SAFETY: `ptr` is a live allocation of `layout.size()` bytes.
    unsafe { core::ptr::write(ptr.as_ptr(), 0xabu8) };
    // SAFETY: read back the byte just written.
    assert_eq!(unsafe { core::ptr::read(ptr.as_ptr()) }, 0xabu8);
    // SAFETY: deallocate exactly the allocation `ptr` came from. On the
    // Windows multi-node path this would be a VirtualFree region, but the
    // single-node fallback (the only reachable path under the themis
    // Windows probe) uses the standard allocator.
    unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) };
}

#[test]
fn first_touch_is_idempotent_and_touches_every_page() {
    let layout = Layout::from_size_align(3 * 4096, 4096).expect("valid layout");
    // SAFETY: fresh allocation, released below.
    let ptr = unsafe { std::alloc::alloc(layout) };
    assert!(!ptr.is_null(), "std alloc succeeds");
    // SAFETY: `ptr` is live for `layout.size()` bytes and writable.
    unsafe { first_touch(ptr, layout.size()) };
    // SAFETY: same range, still live; the first byte of each stride slot was
    // written as zero by the touch.
    for offset in [0usize, 4096, 8192, 12_288] {
        assert_eq!(
            unsafe { core::ptr::read(ptr.add(offset)) },
            0u8,
            "touched byte at offset {offset} reads zero"
        );
    }
    // Idempotence: a second pass must not fault or alter values.
    // SAFETY: same live range.
    unsafe { first_touch(ptr, layout.size()) };
    // SAFETY: deallocate exactly the allocation `ptr` came from.
    unsafe { std::alloc::dealloc(ptr, layout) };
}

#[test]
fn bind_to_node_on_fresh_allocation_is_ok() {
    let layout = Layout::from_size_align(64 * 1024, 4096).expect("valid layout");
    // SAFETY: fresh mmap-backed allocation (unfaulted), released below.
    let ptr = unsafe { std::alloc::alloc(layout) };
    assert!(!ptr.is_null(), "std alloc succeeds");
    // SAFETY: `ptr` is a live allocation of `layout.size()` bytes; on Linux
    // the pages are unfaulted so `mbind(MPOL_BIND | MPOL_MF_STRICT)` to node
    // zero succeeds; elsewhere the call is a documented no-op.
    let result = unsafe { bind_to_node(ptr, layout.size(), NumaNodeId::ZERO) };
    assert!(
        result.is_ok(),
        "binding a fresh allocation to node zero succeeds: {result:?}"
    );
    // SAFETY: deallocate exactly the allocation `ptr` came from.
    unsafe { std::alloc::dealloc(ptr, layout) };
}

#[cfg(target_os = "linux")]
#[test]
fn bind_to_node_rejects_out_of_range_node() {
    let layout = Layout::from_size_align(4096, 4096).expect("valid layout");
    // SAFETY: fresh allocation, released below.
    let ptr = unsafe { std::alloc::alloc(layout) };
    assert!(!ptr.is_null(), "std alloc succeeds");
    // SAFETY: `ptr` is live; the wrapper rejects the id before any kernel
    // call, so this is deterministic on every Linux system.
    let result = unsafe { bind_to_node(ptr, layout.size(), NumaNodeId::new(u32::MAX)) };
    assert!(
        matches!(result, Err(NumaError::InvalidNode { .. })),
        "out-of-range node id is rejected by the wrapper: {result:?}"
    );
    // SAFETY: deallocate exactly the allocation `ptr` came from.
    unsafe { std::alloc::dealloc(ptr, layout) };
}

#[test]
fn tiered_alloc_numa_hint_is_best_effort_and_returns_block() {
    assert_eq!(
        tier_for(PlacementHint::Numa(NumaNodeId::ZERO)),
        MemoryTier::Dram
    );
    scope_tiered::<StandardPolicy, _, _>(|tiered, mut token| {
        let layout = Layout::from_size_align(64, 8).expect("valid layout");
        let block = tiered
            .alloc(&token, layout, PlacementHint::Numa(NumaNodeId::ZERO))
            .expect("Numa hint still routes to the host pool");
        assert_eq!(block.tier(), MemoryTier::Dram);
        // SAFETY: `block` is a live allocation of at least `layout.size()`
        // bytes.
        unsafe { core::ptr::write(block.as_ptr(), 0x5au8) };
        tiered.free(&mut token, block);
    });
}
