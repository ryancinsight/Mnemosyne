//! Per-process and per-thread entropy for the free-list XOR key.
//!
//! The free-list XOR key is split into two independent components:
//!
//! - **Process key** (`PROCESS_KEY`): one random value chosen at first use and
//!   shared across all threads. An adversary that reads one thread's TLS seed
//!   still cannot predict the process key.
//! - **TLS seed** (`get_tls_seed`): a per-thread random value produced
//!   independently from the process key so that knowing the process key is not
//!   sufficient to predict any particular thread's key material.
//!
//! Both values exclude `0` (replaced by a fixed fallback) so the XOR key is
//! always non-trivially mixed into every free-list pointer.
//!
//! This module is the single source of truth for key entropy; callers
//! (`initialize_segment_keys`) read from here and nowhere else.

use core::sync::atomic::{AtomicUsize, Ordering};

melinoe::thread_cached! {
    mod tls_seed: usize;
}

// ── Process key ───────────────────────────────────────────────────────────────

/// Process-wide random component of the free-list XOR key.
///
/// Combined with the per-thread seed in `initialize_segment_keys` so that
/// knowing one thread's TLS seed is insufficient to predict another thread's
/// free-list keys (defense in depth against information-disclosure chains).
///
/// Initialized lazily on the first call to [`get_process_key`] using the same
/// `RandomState` entropy source as [`get_tls_seed`]. Reads after initialization
/// are relaxed loads — the acquire/release pairing at initialization is
/// sufficient: no allocator operation can race on the raw pointer value once
/// it is committed.
static PROCESS_KEY: AtomicUsize = AtomicUsize::new(0);

// Fallback bit patterns are stable on 64-bit and truncated on 32-bit (WASM).
const PROCESS_KEY_FALLBACK: usize = 0xABCD_ABCD_ABCD_ABCDu64 as usize;
const TLS_SEED_FALLBACK: usize = 0xDEAD_BEEF_FACE_FEEDu64 as usize;

/// Returns the process-wide component of the free-list XOR key, initializing
/// it once on the first call.
///
/// `0` is excluded and replaced by `PROCESS_KEY_FALLBACK` so the key is always
/// non-trivially mixed into every free-list pointer.
#[inline(always)]
pub(crate) fn get_process_key() -> usize {
    let existing = PROCESS_KEY.load(Ordering::Relaxed);
    if existing != 0 {
        return existing;
    }
    init_process_key()
}

/// Cold initialization path — called exactly once per process.
#[cold]
#[inline(never)]
fn init_process_key() -> usize {
    use std::hash::{BuildHasher, Hasher};
    let state = std::collections::hash_map::RandomState::new();
    let mut hasher = state.build_hasher();
    // Distinct write value from `get_tls_seed` so the two keys are
    // statistically independent even if `RandomState` reuses internal state.
    hasher.write_usize(usize::MAX);
    let mut key = hasher.finish() as usize;
    if key == 0 {
        key = PROCESS_KEY_FALLBACK;
    }
    // CAS: if another thread raced us, accept their value.
    match PROCESS_KEY.compare_exchange(0, key, Ordering::Release, Ordering::Relaxed) {
        Ok(_) => key,
        Err(existing) => existing,
    }
}

// ── TLS seed ──────────────────────────────────────────────────────────────────

/// Returns the per-thread component of the free-list XOR key, initializing it
/// on the first call for this thread.
///
/// `0` is excluded and replaced by `TLS_SEED_FALLBACK`.
#[inline(always)]
pub(crate) fn get_tls_seed() -> usize {
    tls_seed::get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let state = std::collections::hash_map::RandomState::new();
        let mut hasher = state.build_hasher();
        hasher.write_usize(0);
        let mut seed = hasher.finish() as usize;
        if seed == 0 {
            seed = TLS_SEED_FALLBACK;
        }
        seed
    })
}
