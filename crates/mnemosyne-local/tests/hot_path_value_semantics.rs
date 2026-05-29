//! Value-semantic integration tests for the thread-local allocator hot paths.
//!
//! These run as a separate test binary (its own process, hence its own global
//! segment pool) and assert the properties that micro-optimizations on the
//! allocate / free / page-management hot paths can silently violate:
//!
//! * every returned pointer is non-null and correctly aligned,
//! * `usable_size` never under-reports the request,
//! * concurrently-live blocks are *distinct and non-overlapping* (a write to
//!   one block never disturbs another), and
//! * allocate/free churn (which drives page recycling and segment reclaim)
//!   preserves all of the above.
//!
//! The distinct/non-overlap/round-trip check is the key guard: an
//! unchecked-indexing or size-mapping regression that hands out an overlapping
//! or wrong-class block corrupts the per-block sentinel pattern and fails here,
//! even when each operation "succeeds" in isolation. These exercise the real
//! backend (so they run under `cargo test`, not Miri) and complement the
//! Miri-validated pure-logic unit tests.

use mnemosyne_backend::MemoryBackendWrapper as Backend;
use mnemosyne_core::StandardPolicy as Policy;
use mnemosyne_local::{thread_alloc, thread_free, usable_size};

const ALIGN: usize = 16;

/// Representative sizes spanning the smallest class, several size-class
/// boundaries (`+1` lands in the next class), and the small/large cutoff.
const SIZES: &[usize] = &[
    1, 8, 15, 16, 17, 24, 32, 33, 48, 64, 100, 128, 256, 511, 512, 1000, 1024, 4096, 8192,
];

#[inline]
unsafe fn alloc(size: usize) -> *mut u8 {
    thread_alloc::<Policy, Backend>(size, ALIGN)
}

#[inline]
unsafe fn free(ptr: *mut u8) {
    thread_free::<Policy, Backend>(ptr);
}

/// Allocates many blocks of each size class at once, stamps each with a
/// per-block sentinel over its full requested span, then reads every block
/// back. Any overlap, duplicate pointer, or wrong-size mapping corrupts a
/// sentinel and fails. Pointers are also asserted pairwise-distinct.
#[test]
fn distinct_nonoverlapping_blocks_round_trip_each_size_class() {
    const N: usize = 64;
    for &size in SIZES {
        let mut ptrs = [core::ptr::null_mut::<u8>(); N];
        for (i, slot) in ptrs.iter_mut().enumerate() {
            // Safety: `size` is a valid small request; ALIGN is a power of two.
            let p = unsafe { alloc(size) };
            assert!(!p.is_null(), "alloc({size}) #{i} returned null");
            assert_eq!(p as usize % ALIGN, 0, "alloc({size}) #{i} misaligned");
            // Safety: usable_size accepts a live shim pointer.
            let usable = unsafe { usable_size(p) };
            assert!(
                usable >= size,
                "usable_size {usable} under-reports request {size}"
            );
            // Stamp the whole requested span with a per-block byte.
            let stamp = (i as u8).wrapping_mul(31).wrapping_add(0x5A);
            // Safety: p is valid for `size` writes.
            unsafe { core::ptr::write_bytes(p, stamp, size) };
            *slot = p;
        }

        // Read every block back: overlap/duplication would have clobbered a stamp.
        for (i, &p) in ptrs.iter().enumerate() {
            let stamp = (i as u8).wrapping_mul(31).wrapping_add(0x5A);
            for off in 0..size {
                // Safety: p is live and valid for `size` reads until freed below.
                let got = unsafe { *p.add(off) };
                assert_eq!(
                    got, stamp,
                    "block #{i} (size {size}) corrupted at offset {off}: {got:#x} != {stamp:#x}"
                );
            }
        }

        // Pairwise-distinct pointers (an O(N^2) check; N is small).
        for i in 0..N {
            for j in (i + 1)..N {
                assert_ne!(
                    ptrs[i], ptrs[j],
                    "duplicate pointer handed out for size {size}"
                );
            }
        }

        for &p in &ptrs {
            // Safety: each pointer came from `alloc` above and is freed once.
            unsafe { free(p) };
        }
    }
}

/// Allocate/free churn across mixed sizes drives page recycling and segment
/// reclamation. After each round the freshly handed-out block must still be
/// writable over its full span and `usable_size` must hold — catching
/// metadata corruption that only surfaces on recycled pages/segments.
#[test]
fn alloc_free_churn_preserves_block_integrity() {
    const ROUNDS: usize = 2_000;
    let mut live: [*mut u8; 8] = [core::ptr::null_mut(); 8];
    let mut live_size = [0usize; 8];

    for round in 0..ROUNDS {
        let slot = round % live.len();
        // Free the previous occupant of this slot, if any.
        if !live[slot].is_null() {
            // Verify it survived intact since allocation before freeing.
            let stamp = (slot as u8) ^ 0xC3;
            let size = live_size[slot];
            for off in 0..size {
                // Safety: live[slot] is a block allocated in an earlier round.
                let got = unsafe { *live[slot].add(off) };
                assert_eq!(
                    got, stamp,
                    "recycled-slot block (size {size}) corrupted at {off} on round {round}"
                );
            }
            // Safety: allocated by us, freed once.
            unsafe { free(live[slot]) };
        }

        // Allocate a new block whose size varies across classes by round.
        let size = SIZES[round % SIZES.len()];
        // Safety: valid small request.
        let p = unsafe { alloc(size) };
        assert!(
            !p.is_null(),
            "churn alloc({size}) returned null on round {round}"
        );
        assert!(
            unsafe { usable_size(p) } >= size,
            "churn usable_size under-reports"
        );
        let stamp = (slot as u8) ^ 0xC3;
        // Safety: p is valid for `size` writes.
        unsafe { core::ptr::write_bytes(p, stamp, size) };
        live[slot] = p;
        live_size[slot] = size;
    }

    for &p in &live {
        if !p.is_null() {
            // Safety: each live pointer was allocated by us and is freed once.
            unsafe { free(p) };
        }
    }
}

/// A zero-size request returns null (Mnemosyne does not hand out a unique
/// sentinel for `size == 0` at this layer), and must not panic — this directly
/// guards the validator-underflow regression class at the public entry point.
#[test]
fn zero_size_request_returns_null_without_panicking() {
    // Safety: zero size is rejected by validation; ALIGN is valid.
    let p = unsafe { alloc(0) };
    assert!(p.is_null(), "zero-size allocation must return null");
}
