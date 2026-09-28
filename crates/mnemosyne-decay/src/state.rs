//! Process-global decay state: the spawn/generation counters and the two
//! `OnceLock` condition-variable pairs the lifecycle handshake uses.

use core::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex, OnceLock};

/// Whether a decay worker thread has been spawned.
pub(crate) static SPAWNED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);
/// Generation of the most recently spawned worker.
pub(crate) static DECAY_WORKER_GENERATION: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
/// Generation whose worker has published its final exit.
pub(crate) static DECAY_FINAL_EXIT_GENERATION: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
/// Number of completed decay sweeps observed by this process.
pub(crate) static DECAY_STEP_GENERATION: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);
/// Step/shutdown signal shared with external observers.
pub(crate) static DECAY_EVENT: OnceLock<(Mutex<()>, Condvar)> = OnceLock::new();
/// Immediate-sweep wake signal shared with the worker.
pub(crate) static DECAY_WAKE_EVENT: OnceLock<(Mutex<bool>, Condvar)> = OnceLock::new();

/// Returns whether the current worker generation has published its final exit.
pub(crate) fn decay_shutdown_published() -> bool {
    let worker_generation = DECAY_WORKER_GENERATION.load(Ordering::Acquire);
    let final_exit_generation = DECAY_FINAL_EXIT_GENERATION.load(Ordering::Acquire);
    !SPAWNED.load(Ordering::Acquire) && final_exit_generation >= worker_generation
}
