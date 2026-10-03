//! Frees through pointers whose provenance the caller narrowed to the
//! allocation.
//!
//! A program reaches its allocations through references and `Box`es, which
//! retag the pointer to the allocation's bytes alone. The allocator's metadata
//! lies outside those bytes, so the free path reaches it through the registered
//! mapping, never the caller's provenance (ADR 0012); the case fails Tree
//! Borrows on a tree that reads page metadata through the caller's pointer.
//! The narrowed pointer still covers its whole block, which `thread_free`
//! requires: the free-list link is written through it.

use super::*;

/// Narrows `ptr` to its first `len` bytes, as `&mut [u8]` or `Box<[u8]>`
/// does: the result's provenance covers those bytes and nothing else.
///
/// # Safety
///
/// `ptr` must be a live allocation of at least `len` bytes that the caller
/// owns exclusively.
unsafe fn narrowed(ptr: *mut u8, len: usize) -> *mut u8 {
    // SAFETY: forwarded from this function's contract.
    unsafe { core::slice::from_raw_parts_mut(ptr, len) }.as_mut_ptr()
}

/// Fills `len` bytes at `ptr` with `byte`.
///
/// # Safety
///
/// `ptr` must be valid for writes of `len` bytes.
unsafe fn fill(ptr: *mut u8, len: usize, byte: u8) {
    // SAFETY: forwarded from this function's contract.
    unsafe { core::ptr::write_bytes(ptr, byte, len) };
}

/// Returns whether all `len` bytes at `ptr` equal `byte`.
///
/// # Safety
///
/// `ptr` must be valid for reads of `len` bytes.
unsafe fn holds(ptr: *const u8, len: usize, byte: u8) -> bool {
    // SAFETY: forwarded from this function's contract.
    unsafe { core::slice::from_raw_parts(ptr, len) }
        .iter()
        .all(|&value| value == byte)
}

const SMALL_SIZE: usize = 32;
const SMALL_BLOCKS: u8 = 48;

#[test]
fn small_blocks_freed_through_narrowed_pointers_are_reused_without_aliasing() {
    let mut first = [core::ptr::null_mut::<u8>(); SMALL_BLOCKS as usize];
    for (slot, byte) in first.iter_mut().zip(0..SMALL_BLOCKS) {
        // SAFETY: non-zero size, power-of-two alignment.
        let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(SMALL_SIZE, 8) };
        assert!(!ptr.is_null(), "small allocation {byte} failed");
        // SAFETY: `ptr` is a fresh, exclusively owned `SMALL_SIZE`-byte block.
        // `SMALL_SIZE` is a size class, so the narrowed pointer still covers
        // the whole block, as `thread_free` requires.
        let ptr = unsafe { narrowed(ptr, SMALL_SIZE) };
        // SAFETY: as above.
        unsafe { fill(ptr, SMALL_SIZE, byte) };
        *slot = ptr;
    }
    for ptr in first {
        // SAFETY: each pointer is a live block of this allocator, freed once.
        unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
    }

    // The blocks just freed are handed out again. Each is filled with its own
    // byte while all are live, so a free list corrupted by the narrowed frees
    // shows up as two allocations sharing bytes.
    let mut second = [core::ptr::null_mut::<u8>(); SMALL_BLOCKS as usize];
    for (slot, byte) in second.iter_mut().zip(0..SMALL_BLOCKS) {
        // SAFETY: non-zero size, power-of-two alignment.
        let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(SMALL_SIZE, 8) };
        assert!(!ptr.is_null(), "small reallocation {byte} failed");
        // SAFETY: `ptr` is a fresh, exclusively owned `SMALL_SIZE`-byte block.
        unsafe { fill(ptr, SMALL_SIZE, byte) };
        *slot = ptr;
    }
    for (&ptr, byte) in second.iter().zip(0..SMALL_BLOCKS) {
        // SAFETY: every block is live and `SMALL_SIZE` bytes long.
        assert!(
            unsafe { holds(ptr, SMALL_SIZE, byte) },
            "block {byte} at {ptr:?} was overwritten by another live allocation"
        );
    }
    for ptr in second {
        // SAFETY: each pointer is a live block of this allocator, freed once.
        unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
    }
}

/// A request served by its own mapping, so the free reads the owning segment
/// from the slot before the payload, outside the caller's bytes (ADR 0012).
const HUGE_SIZE: usize = 4 * 1024 * 1024;

#[test]
fn huge_blocks_freed_through_narrowed_pointers_resolve_their_segment() {
    let _guard = crate::local_alloc::TEST_LOCK
        .lock()
        .expect("invariant: a panicking test does not hold the lock");
    // SAFETY: non-zero size, power-of-two alignment.
    let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(HUGE_SIZE, 8) };
    assert!(!ptr.is_null(), "huge allocation failed");
    // SAFETY: `ptr` is the live huge allocation just returned.
    let block_len = unsafe { usable_size(ptr) };
    assert!(
        block_len >= HUGE_SIZE,
        "usable_size = {block_len} is below the huge request {HUGE_SIZE}"
    );
    // SAFETY: `ptr` is exclusively owned and valid for `block_len` bytes; the
    // narrowed pointer covers the whole block, as `thread_free` requires, and
    // excludes the back-pointer slot before it.
    let ptr = unsafe { narrowed(ptr, block_len) };
    // SAFETY: as above.
    assert_eq!(
        unsafe { usable_size(ptr) },
        block_len,
        "the narrowed pointer resolved a different segment"
    );
    // SAFETY: a live block of this allocator, freed once.
    unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
    // The huge pool retains the mapping by design; drain it so the run ends
    // with no mapping Miri reports as leaked.
    // SAFETY: the test lock is held, so no other test mutates the pool.
    unsafe { mnemosyne_arena::purge_segment_pool::<MemoryBackendWrapper>() };
}
