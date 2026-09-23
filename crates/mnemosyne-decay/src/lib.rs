//! Background decay and reclamation for Mnemosyne arenas.
//!
//! Segments freed by the allocator are not returned to the operating system
//! immediately: holding them lets a subsequent allocation of the same size
//! class reuse the mapping without a syscall. This crate owns the opposite
//! side of that trade — it periodically purges segments that have gone cold,
//! so a burst of allocation does not pin resident memory indefinitely.
//!
//! [`init_decay_engine`] lazily spawns the worker thread. [`decay_step`]
//! performs one sweep and is public so a caller with its own scheduler can
//! drive reclamation without the background thread. [`request_decay_step`]
//! wakes an already-running worker immediately.

#![deny(missing_docs)]

mod engine;
mod events;
mod orphan;

use core::sync::atomic::Ordering;
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::options::PURGE_CADENCE_MS;
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use events::{decay_step_event, publish_decay_step, wake_decay_worker};

// ── Process-global state ──────────────────────────────────────────────────────

static SPAWNED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static DECAY_WORKER_GENERATION: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
static DECAY_FINAL_EXIT_GENERATION: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
static DECAY_STEP_GENERATION: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
static DECAY_EVENT: OnceLock<(Mutex<()>, Condvar)> = OnceLock::new();
static DECAY_WAKE_EVENT: OnceLock<(Mutex<bool>, Condvar)> = OnceLock::new();

// ── Public API ────────────────────────────────────────────────────────────────

/// Triggers background decay thread initialization.
///
/// Lazily spawns a background worker thread on options initialization if
/// `MNEMOSYNE_PURGE_CADENCE_MS` is non-zero.
pub fn init_decay_engine() {
    let cadence = PURGE_CADENCE_MS.load(Ordering::Acquire);
    if cadence == 0 {
        wake_decay_worker();
        return;
    }

    // Unconditional AcqRel RMW (no plain-load fast path): every spawn attempt
    // and the purger's shutdown handshake then meet on SPAWNED's single
    // modification order, carrying the caller's preceding PURGE_CADENCE_MS
    // store into the dying thread's re-check. A plain `load` fast path could
    // skip the spawn without creating that edge and lose the cadence store.
    if cadence > 0 && !SPAWNED.swap(true, Ordering::AcqRel) {
        let worker_generation = DECAY_WORKER_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        thread::Builder::new()
            .name("mnemosyne-decay".to_string())
            .spawn(move || {
                engine::decay_thread_loop(cadence, worker_generation);
            })
            .expect("Failed to spawn mnemosyne-decay thread");
    }
}

/// Requests one immediate background decay sweep.
///
/// Requests are coalesced while the worker is asleep. If no worker is active,
/// the request remains pending until the next worker starts, which keeps a
/// configuration change from losing a requested sweep during the restart
/// handshake. The request does not run a sweep synchronously; observe its
/// completion with [`wait_for_decay_step`].
pub fn request_decay_step() {
    wake_decay_worker();
}

/// Returns the number of completed decay sweeps observed by this process.
#[must_use]
pub fn decay_step_generation() -> usize {
    DECAY_STEP_GENERATION.load(Ordering::Acquire)
}

/// Waits for a decay sweep after a previously observed generation.
///
/// A `false` result means the supplied timeout elapsed before a later
/// generation was observed.
#[must_use]
pub fn wait_for_decay_step(previous: usize, timeout: Duration) -> bool {
    let (lock, event) = decay_step_event();
    let guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let (_guard, _timeout) = event
        .wait_timeout_while(guard, timeout, |_| {
            DECAY_STEP_GENERATION.load(Ordering::Acquire) <= previous
        })
        .unwrap_or_else(|p| p.into_inner());
    DECAY_STEP_GENERATION.load(Ordering::Acquire) > previous
}

/// Waits for the background worker to stop after cadence reaches zero.
///
/// A `true` result means no decay worker is active and the current worker
/// generation has published its final exit. A `false` result means the
/// supplied timeout elapsed before that publication.
#[must_use]
pub fn wait_for_decay_shutdown(timeout: Duration) -> bool {
    let (lock, event) = decay_step_event();
    let guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    let (_guard, _timeout) = event
        .wait_timeout_while(guard, timeout, |_| !decay_shutdown_published())
        .unwrap_or_else(|p| p.into_inner());
    decay_shutdown_published()
}

/// Returns whether the current worker generation has published its final exit.
fn decay_shutdown_published() -> bool {
    let worker_generation = DECAY_WORKER_GENERATION.load(Ordering::Acquire);
    let final_exit_generation = DECAY_FINAL_EXIT_GENERATION.load(Ordering::Acquire);
    !SPAWNED.load(Ordering::Acquire) && final_exit_generation >= worker_generation
}

/// Executes a single decay cycle across all active memory backends.
///
/// Sweeps the global orphan pool for each backend, draining cross-thread
/// frees in idle segments and releasing them back to the OS if empty. Also
/// purges the global segment pool to drop retained free mappings.
///
/// # Closed-set invariant
///
/// This list must name every backend whose segment/orphan pools production
/// code can populate. A new backend gaining a `LocalAllocatorSelector` impl
/// **must** be added here, or its orphaned segments and retained mappings are
/// never reclaimed. `DefaultBackend` is intentionally absent: it has no
/// production selector impl and its pools stay empty.
pub fn decay_step() {
    decay_step_for_backend::<mnemosyne_backend::MemoryBackendWrapper>();
    decay_step_for_backend::<mnemosyne_backend::CudaUnifiedBackend>();
    decay_step_for_backend::<mnemosyne_backend::CudaDeviceBackend>();
    decay_step_for_backend::<mnemosyne_backend::CudaHbmBackend>();
    decay_step_for_backend::<mnemosyne_backend::CudaGddrBackend>();
    decay_step_for_backend::<mnemosyne_backend::CudaHostPinnedBackend>();
    publish_decay_step();
}

fn decay_step_for_backend<B: HasSegmentPool>() {
    orphan::drain_orphan_pool::<B>();
    // SAFETY: maintenance step only touches backend-owned retained segments
    // for `B`; no caller-provided pointers are involved.
    unsafe {
        mnemosyne_arena::purge_segment_pool::<B>();
    }
}
