//! Event primitives driving the background decay worker.
//!
//! Two `OnceLock<(Mutex, Condvar)>` pairs coordinate the decay lifecycle:
//!
//! - **`DECAY_EVENT`** — signals completed sweeps and worker shutdown to
//!   external observers ([`wait_for_decay_step`][super::wait_for_decay_step],
//!   [`wait_for_decay_shutdown`][super::wait_for_decay_shutdown]).
//! - **`DECAY_WAKE_EVENT`** — wakes the worker thread out of its sleep when an
//!   immediate sweep is requested ([`super::request_decay_step`]).

use core::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use super::{DECAY_EVENT, DECAY_FINAL_EXIT_GENERATION, DECAY_STEP_GENERATION, DECAY_WAKE_EVENT};

/// Returns the shared (Mutex, Condvar) used for step and shutdown signals.
pub(super) fn decay_step_event() -> &'static (Mutex<()>, Condvar) {
    DECAY_EVENT.get_or_init(|| (Mutex::new(()), Condvar::new()))
}

/// Returns the shared (Mutex<bool>, Condvar) used for worker wake-ups.
pub(super) fn decay_wake_event() -> &'static (Mutex<bool>, Condvar) {
    DECAY_WAKE_EVENT.get_or_init(|| (Mutex::new(false), Condvar::new()))
}

/// Signals the background worker to wake from its timed sleep immediately.
///
/// Requests are coalesced: if the worker is already awake the boolean flag
/// is set for the next sleep instead of queuing a second wake.
pub(super) fn wake_decay_worker() {
    let (lock, event) = decay_wake_event();
    let mut wake_requested = lock.lock().unwrap_or_else(|p| p.into_inner());
    *wake_requested = true;
    event.notify_one();
}

/// Blocks the calling thread until the background worker wakes or the interval elapses.
///
/// Resets the wake flag before returning so each wake is consumed once.
pub(super) fn wait_for_decay_interval(interval_ms: u64) {
    let (lock, event) = decay_wake_event();
    let wake_requested = lock.lock().unwrap_or_else(|p| p.into_inner());
    let (mut wake_requested, _timeout) = event
        .wait_timeout_while(
            wake_requested,
            Duration::from_millis(interval_ms),
            |requested| !*requested,
        )
        .unwrap_or_else(|p| p.into_inner());
    *wake_requested = false;
}

/// Increments the step generation counter and wakes step-waiters.
pub(super) fn publish_decay_step() {
    let (lock, event) = decay_step_event();
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    DECAY_STEP_GENERATION.fetch_add(1, Ordering::Release);
    event.notify_all();
}

/// Records the final exit of the worker at `worker_generation` and wakes shutdown-waiters.
pub(super) fn publish_decay_worker_exit(worker_generation: usize) {
    let (lock, event) = decay_step_event();
    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    DECAY_FINAL_EXIT_GENERATION.fetch_max(worker_generation, Ordering::Release);
    event.notify_all();
}
