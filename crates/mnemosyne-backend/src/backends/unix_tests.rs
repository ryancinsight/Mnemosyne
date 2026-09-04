//! Tests for the Unix memory backend.
//!
//! A sidecar rather than an inline module: `unix.rs` is the backend, and its
//! tests were a quarter of its length. The Linux gate stays at the
//! declaration, where it was.

extern crate std;

use super::*;
use crate::recorders::backend_memory_stats;
use mnemosyne_core::MemoryBackend;

/// Serializes the hint-counter snapshots. `hugepage_hint_calls` is a
/// process-global; nextest gives each test its own process, but a plain
/// `cargo test` run shares one, where a sibling's segment-sized mapping
/// could land between a test's two reads and turn an exact delta into a
/// flake.
static HINT_COUNTER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Reads the process-global hint counter.
fn hugepage_hints() -> usize {
    backend_memory_stats().hugepage_hint_calls
}

// The hugepage hint this exercises is itself `not(miri)`, and so is the
// `SEGMENT_SIZE` import it needs, so under Miri the test would neither
// compile nor have a subject. Gated to match the code it covers.
#[cfg(not(miri))]
#[test]
fn segment_sized_allocation_survives_hugepage_hint() {
    // The MADV_HUGEPAGE hint is purely advisory: a Linux kernel that
    // ignores it must still produce a mapping that allocate/deallocate
    // can round-trip without error, and reads/writes against the
    // returned region must succeed. This regression-guards the hint
    // path against accidentally treating a benign EINVAL from the
    // advice as a fatal mapping failure.
    let _guard = HINT_COUNTER_LOCK
        .lock()
        .expect("hugepage hint counter lock was poisoned");
    let size = SEGMENT_SIZE;
    let before = hugepage_hints();
    // SAFETY: SEGMENT_SIZE is a non-zero power-of-two multiple of the
    // system page size, satisfying the allocate contract.
    let ptr = unsafe { UnixBackend::allocate(size) };
    assert!(!ptr.is_null(), "segment-sized mapping must succeed");
    assert_eq!(
        hugepage_hints(),
        before + 1,
        "a mapping of exactly SEGMENT_SIZE must receive the hugepage hint"
    );

    // Touch the boundary bytes to confirm the entire region is mapped.
    // SAFETY: ptr covers [0, size) bytes per the allocate contract.
    unsafe {
        ptr.write_volatile(0xAA);
        ptr.add(size - 1).write_volatile(0x55);
        assert_eq!(ptr.read_volatile(), 0xAA);
        assert_eq!(ptr.add(size - 1).read_volatile(), 0x55);
    }

    // SAFETY: ptr is the exact base of the size-byte mapping.
    let released = unsafe { UnixBackend::deallocate(ptr, size) };
    assert!(
        released,
        "munmap reported failure for segment-sized mapping"
    );
}

// The hugepage hint this exercises is itself `not(miri)`, and so is the
// `SEGMENT_SIZE` import it needs, so under Miri the test would neither
// compile nor have a subject. Gated to match the code it covers.
#[cfg(not(miri))]
#[test]
fn sub_segment_allocation_skips_hugepage_hint() {
    // Mappings smaller than SEGMENT_SIZE must not receive the hint
    // (it would be unaligned to the THP boundary and produce noise in
    // kernel logs). This test confirms the path still allocates,
    // populates the boundary bytes, and releases cleanly.
    let _guard = HINT_COUNTER_LOCK
        .lock()
        .expect("hugepage hint counter lock was poisoned");
    let size = PAGE_SIZE_FALLBACK;
    let before = hugepage_hints();
    // SAFETY: size is a non-zero multiple of the system page size.
    let ptr = unsafe { UnixBackend::allocate(size) };
    assert!(!ptr.is_null());
    assert_eq!(
        hugepage_hints(),
        before,
        "a sub-SEGMENT_SIZE mapping must not receive the hugepage hint"
    );

    unsafe {
        ptr.write_volatile(0xAA);
        ptr.add(size - 1).write_volatile(0x55);
    }

    let released = unsafe { UnixBackend::deallocate(ptr, size) };
    assert!(released);
}

// The hint is compiled out under Miri, so the counter cannot move there.
// Gated to match the code it covers, like its two siblings.
#[cfg(not(miri))]
#[test]
fn large_non_multiple_allocation_receives_hugepage_hint() {
    // Mappings larger than or equal to SEGMENT_SIZE that are not multiples of
    // SEGMENT_SIZE must receive the hint and round-trip correctly.
    // We use 3 MiB (which is 1.5 * SEGMENT_SIZE).
    let _guard = HINT_COUNTER_LOCK
        .lock()
        .expect("hugepage hint counter lock was poisoned");
    let size = 3 * 1024 * 1024;
    let before = hugepage_hints();
    // SAFETY: size is a non-zero multiple of the system page size.
    let ptr = unsafe { UnixBackend::allocate(size) };
    assert!(!ptr.is_null(), "large mapping must succeed");
    assert_eq!(
        hugepage_hints(),
        before + 1,
        "a mapping above SEGMENT_SIZE must receive the hugepage hint even \
         when it is not a whole multiple of it"
    );

    unsafe {
        ptr.write_volatile(0xAA);
        ptr.add(size - 1).write_volatile(0x55);
        assert_eq!(ptr.read_volatile(), 0xAA);
        assert_eq!(ptr.add(size - 1).read_volatile(), 0x55);
    }

    // SAFETY: `ptr`/`size` are exactly what the matching `allocate`
    // returned above, and the mapping has not been released yet.
    let released = unsafe { UnixBackend::deallocate(ptr, size) };
    assert!(
        released,
        "munmap reported failure for large non-multiple mapping"
    );
}

/// 4 KiB is the system page size on every Linux configuration this test
/// runs against; explicit to avoid importing `mnemosyne_core::PAGE_SIZE`
/// (which is the allocator-domain page size of 64 KiB, not the OS page).
const PAGE_SIZE_FALLBACK: usize = 4096;
