//! Frees, resizes, and size queries through pointers whose provenance the
//! caller narrowed to the allocation.
//!
//! A program reaches its allocations through references and `Box`es, which
//! retag the pointer to the allocation's bytes alone; `Box<[T]>::into_vec`
//! hands exactly such a pointer to `dealloc`. The allocator's metadata lies
//! outside those bytes, so it must not be reached through the caller's
//! provenance (MN-LOCAL-MIRI-UB). These cases drive every entry point with a
//! narrowed pointer, so the crate's own Miri job checks that contract under
//! both borrow models; the native assertions pin the values the paths return.

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

/// A request narrower than the 8-byte free-list link (ADR 0012).
const TINY_SIZE: usize = 4;

/// Blocks in one smallest-class page: a fresh page is bump-allocated before
/// its free list is read, so reuse needs a page's worth of frees.
const TINY_BLOCKS: usize =
    mnemosyne_core::constants::PAGE_SIZE / mnemosyne_core::constants::MIN_BLOCK_SIZE;

#[test]
fn blocks_freed_through_pointers_narrower_than_the_link_are_reused_without_aliasing() {
    // Each free covers only `TINY_SIZE` bytes of its 16-byte block, so the
    // link is written partly through the caller's pointer and partly through
    // the mapping's; reuse then reads it whole through the mapping's.
    let alloc_tiny = |round: &str| {
        let mut blocks = ::std::vec::Vec::with_capacity(TINY_BLOCKS);
        for index in 0..TINY_BLOCKS {
            // SAFETY: non-zero size, power-of-two alignment.
            let ptr = unsafe {
                thread_alloc_layout::<StandardPolicy, MemoryBackendWrapper>(TINY_SIZE, TINY_SIZE)
            };
            assert!(!ptr.is_null(), "{round} tiny allocation {index} failed");
            // SAFETY: `ptr` is a fresh, exclusively owned `TINY_SIZE`-byte block.
            let ptr = unsafe { narrowed(ptr, TINY_SIZE) };
            // SAFETY: as above. The byte pattern is the index modulo 256.
            unsafe { fill(ptr, TINY_SIZE, index.to_le_bytes()[0]) };
            blocks.push(ptr);
        }
        blocks
    };
    let free_tiny = |blocks: &[*mut u8]| {
        for &ptr in blocks {
            // SAFETY: each pointer is a live block allocated with this layout
            // and freed once.
            unsafe {
                thread_free_layout::<StandardPolicy, MemoryBackendWrapper>(
                    ptr, TINY_SIZE, TINY_SIZE,
                );
            };
        }
    };

    let first = alloc_tiny("first");
    free_tiny(&first);
    let second = alloc_tiny("second");
    for (index, &ptr) in second.iter().enumerate() {
        // SAFETY: every block is live and `TINY_SIZE` bytes long.
        assert!(
            unsafe { holds(ptr, TINY_SIZE, index.to_le_bytes()[0]) },
            "block {index} at {ptr:?} was overwritten by another live allocation"
        );
    }
    let freed: ::std::collections::BTreeSet<usize> = first.iter().map(|ptr| ptr.addr()).collect();
    let reused = second
        .iter()
        .filter(|ptr| freed.contains(&ptr.addr()))
        .count();
    assert!(
        reused > 0,
        "no block freed through a narrowed pointer was handed out again"
    );
    free_tiny(&second);
}

#[test]
fn in_place_realloc_through_narrowed_pointer_returns_a_pointer_covering_the_new_size() {
    const OLD_SIZE: usize = 24;
    // SAFETY: non-zero size, power-of-two alignment.
    let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(OLD_SIZE, 8) };
    assert!(!ptr.is_null(), "small allocation failed");
    // SAFETY: `ptr` is a live block of this allocator.
    let block_size = unsafe { usable_size(ptr) };
    assert!(
        block_size > OLD_SIZE,
        "the {OLD_SIZE}-byte request must leave slack in its {block_size}-byte block"
    );
    // SAFETY: `ptr` is a fresh, exclusively owned `OLD_SIZE`-byte block.
    let ptr = unsafe { narrowed(ptr, OLD_SIZE) };
    // SAFETY: as above.
    unsafe { fill(ptr, OLD_SIZE, 0x5A) };

    let layout = core::alloc::Layout::from_size_align(OLD_SIZE, 8).unwrap();
    // SAFETY: `ptr`/`layout` describe the live allocation; `block_size` is
    // non-zero.
    let grown =
        unsafe { thread_realloc::<StandardPolicy, MemoryBackendWrapper>(ptr, layout, block_size) };
    assert_eq!(
        grown, ptr,
        "growing within the block's own stride must resize in place"
    );
    // The result is the new allocation: it must cover the grown bytes even
    // though the pointer passed in covered only the old ones.
    // SAFETY: `grown` is live for `block_size` bytes.
    unsafe { fill(grown.add(OLD_SIZE), block_size - OLD_SIZE, 0xC3) };
    // SAFETY: as above.
    assert!(
        unsafe { holds(grown, OLD_SIZE, 0x5A) },
        "the in-place resize lost the original contents"
    );
    // SAFETY: as above.
    assert!(unsafe { holds(grown.add(OLD_SIZE), block_size - OLD_SIZE, 0xC3) });
    // SAFETY: `grown` is a live block of this allocator, freed once.
    unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(grown) };
}

#[test]
fn huge_allocation_sized_and_freed_through_narrowed_pointer() {
    let _guard = crate::local_alloc::TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");
    const HUGE_SIZE: usize = 4 * 1024 * 1024;
    // SAFETY: non-zero size, power-of-two alignment.
    let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(HUGE_SIZE, 8) };
    assert!(!ptr.is_null(), "huge allocation failed");
    // SAFETY: `ptr` is a fresh, exclusively owned `HUGE_SIZE`-byte allocation.
    let ptr = unsafe { narrowed(ptr, HUGE_SIZE) };

    // The back-pointer slot before the payload is outside the narrowed bytes.
    // SAFETY: `ptr` is a live allocation of this allocator.
    let reported = unsafe { usable_size(ptr) };
    assert!(
        reported >= HUGE_SIZE,
        "usable_size = {reported} is below the huge request {HUGE_SIZE}"
    );
    // SAFETY: `ptr` is a live allocation of this allocator, freed once.
    unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };

    // The huge pool retains the mapping by design; drain it so the run ends
    // with no mapping Miri would report as leaked.
    // SAFETY: the test lock is held, so no other test mutates the pool.
    unsafe { mnemosyne_arena::purge_segment_pool::<MemoryBackendWrapper>() };
}
