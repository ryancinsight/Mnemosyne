//! Integration test for the Phase 4 dealloc-branch instrumentation probe.
//!
//! Drives `thread_alloc` / `thread_free_layout` against the real backend
//! and asserts that the [`dealloc_counters`] snapshot reflects the
//! bookkeeping the probe claims to record. Cargo-feature gated: the
//! probe module only exists when `dealloc-probe` is enabled, so this
//! test binary is compiled only under that feature.
//!
//! # What it pins
//!
//! 1. `total() == N` after N alloc-and-free round trips — i.e. every
//!    dealloc falls into exactly one of the five committed arms. A
//!    missed arm would lower `total < N`; a double-arm would surface
//!    as an out-of-class bump in a single counter, which the equality
//!    check on `(in_place + active_free_last_block + full_to_active +
//!    huge + cold_or_recursing)` catches.
//! 2. The five-arm array is non-empty under the probe feature (sanity
//!    that wiring isn't accidentally elided by the cfg gate).
//! 3. `reset()` returns the counters to a clean slate so consumers
//!    can scope a fresh measurement window without leaking state from
//!    a prior test binary.

#![cfg(feature = "dealloc-probe")]

use mnemosyne_backend::MemoryBackendWrapper as Backend;
use mnemosyne_core::StandardPolicy as Policy;
use mnemosyne_local::dealloc_counters::{reset, snapshot, total, DeallocPath};
use mnemosyne_local::{thread_alloc, thread_free_layout};

const ALIGN: usize = 16;
const BLOCK_SIZE: usize = 64;
const N: usize = 256;

/// Allocate and free `N` small blocks through the
/// `thread_alloc` / `thread_free_layout` pair, asserting every dealloc
/// is captured by the layout-proven same-owner small branch. The
/// allocation count stays below one page of 64-byte blocks, so the
/// current-segment condition keeps every free in the in-place arm.
#[test]
fn dealloc_probe_records_layout_small_frees_as_in_place() {
    reset();
    assert_eq!(
        total(),
        0,
        "freshly reset probe should report zero recorded deallocations"
    );

    let mut ptrs = [core::ptr::null_mut::<u8>(); N];
    for (i, slot) in ptrs.iter_mut().enumerate() {
        // Safety: `BLOCK_SIZE` is a valid small request and `ALIGN` is a
        // power of two. `thread_alloc` returns null only on
        // out-of-memory, which we treat as a hard failure.
        let p = unsafe { thread_alloc::<Policy, Backend>(BLOCK_SIZE, ALIGN) };
        assert!(!p.is_null(), "alloc #{i} returned null");
        *slot = p;
    }

    for (i, &p) in ptrs.iter().enumerate() {
        // Stamp before freeing so a write past the payload on a
        // wrong-class mapping would be caught by the assertion below.
        // Safety: p is valid for `BLOCK_SIZE` writes.
        unsafe { core::ptr::write_bytes(p, 0xA5, BLOCK_SIZE) };
        // Safety: each pointer was returned by `thread_alloc` above and
        // is freed exactly once; size/align matched the alloc request.
        unsafe { thread_free_layout::<Policy, Backend>(p, BLOCK_SIZE, ALIGN) };
        let _ = i;
    }

    let snap = snapshot();
    assert_eq!(
        snap.len(),
        5,
        "snapshot should expose the five commit arms (HugeClassifier, \
         InPlaceSmall, ActiveFreeLastBlock, FullToActive, ColdOrRecursing)"
    );

    let mut counts = [0_u64; DeallocPath::COUNT];
    for (path, _name, count) in snap {
        counts[path.index()] = count;
    }

    let sum: u64 = counts.iter().copied().sum();
    assert_eq!(
        sum,
        N as u64,
        "every alloc/free pair should commit exactly one branch \
         (recorded {sum}, expected {n}); a missed arm will under-count \
         and a recorded total > N means multiple arms fired from one call",
        sum = sum,
        n = N,
    );
    assert_eq!(
        counts[DeallocPath::InPlaceSmall.index()],
        N as u64,
        "layout-proven same-owner small frees should stay on the in-place path"
    );
    assert_eq!(
        counts[DeallocPath::HugeClassifier.index()],
        0,
        "layout-proven small frees must not hit the large/huge classifier"
    );
    assert_eq!(
        counts[DeallocPath::ColdOrRecursing.index()],
        0,
        "same-thread non-reentrant frees must not fall back to the cold path"
    );
}

/// After a `reset()` the snapshot reports zero on every arm. This
/// guards the boundary contract that callers (e.g., the `bench_*`
/// A/B scripts) lean on to scope a fresh measurement window.
#[test]
fn dealloc_probe_reset_clears_every_arm() {
    // Pre-populate every counter through the public API and assert
    // the start state is non-zero so the post-reset assertion below
    // is meaningful (catches a regression in which reset() runs
    // before record() ever fires).
    use mnemosyne_local::dealloc_counters::record;

    for path in DeallocPath::ALL {
        record(path);
    }
    assert!(total() >= DeallocPath::ALL.len() as u64);

    reset();
    for (path, _name, count) in snapshot() {
        assert_eq!(count, 0, "{path:?} counter not zero after reset()");
    }
    assert_eq!(total(), 0);
}
