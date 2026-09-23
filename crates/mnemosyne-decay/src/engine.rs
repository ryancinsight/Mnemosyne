//! Background decay thread loop with adaptive interval.
//!
//! `decay_thread_loop` runs on the dedicated `mnemosyne-decay` thread. It
//! alternates between sleeping for an adaptive interval and executing one
//! [`decay_step`][super::decay_step]. The interval doubles when a step
//! releases nothing (idle backing-off) and halves when a step releases more
//! than [`ADAPTIVE_SPEED_UP_THRESHOLD`] bytes (active). The configured
//! `MNEMOSYNE_PURGE_CADENCE_MS` acts as the steady-state target, clamped to
//! `[base/4, base*10]` so adaptation never escapes the operator's intent by
//! more than one order of magnitude.

use core::sync::atomic::Ordering;
use mnemosyne_core::options::PURGE_CADENCE_MS;

use super::events::{publish_decay_worker_exit, wait_for_decay_interval};
use super::{SPAWNED, decay_step};

/// Upper bound on the adaptive sleep interval.
const ADAPTIVE_MAX_MS: u64 = 5_000;

/// If a step releases more than this many bytes, the interval is halved.
const ADAPTIVE_SPEED_UP_THRESHOLD: usize = 1 << 20; // 1 MiB

/// Returns the cumulative bytes returned to the OS by the current process.
///
/// The difference across two calls measures how much a single
/// [`decay_step`][super::decay_step] released.
#[inline]
fn os_returned_bytes() -> usize {
    let s = mnemosyne_backend::backend_memory_stats();
    s.decommit_bytes.wrapping_add(s.page_reset_bytes)
}

/// Entry point for the background decay worker thread.
///
/// Runs until `MNEMOSYNE_PURGE_CADENCE_MS` is set to zero and the shutdown
/// handshake completes. Each iteration sleeps for the current adaptive
/// interval, then calls `decay_step` and adjusts the next interval based on
/// how many bytes were freed.
pub(super) fn decay_thread_loop(initial_cadence: usize, worker_generation: usize) {
    let mut current_interval = initial_cadence as u64;

    loop {
        wait_for_decay_interval(current_interval);

        let before = os_returned_bytes();
        decay_step();
        let freed = os_returned_bytes().wrapping_sub(before);

        let base = PURGE_CADENCE_MS.load(Ordering::Acquire);
        if base == 0 {
            // Shutdown handshake: release the SPAWNED claim, then re-check
            // if a new cadence was set concurrently and restart if so.
            let was_spawned = SPAWNED.swap(false, Ordering::AcqRel);
            debug_assert!(
                was_spawned,
                "decay purger exiting without holding the SPAWNED claim"
            );
            let cadence = PURGE_CADENCE_MS.load(Ordering::Acquire);
            if cadence != 0
                && SPAWNED
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                current_interval = cadence as u64;
                continue;
            }
            break;
        }

        // Adaptive interval update (four-sided clamp within one decade of base).
        let base_u64 = base as u64;
        current_interval = if freed == 0 {
            current_interval
                .saturating_mul(2)
                .min(ADAPTIVE_MAX_MS)
                .min(base_u64.saturating_mul(10))
        } else if freed > ADAPTIVE_SPEED_UP_THRESHOLD {
            (current_interval / 2).max(1).max(base_u64 / 4)
        } else {
            base_u64
        };
    }

    publish_decay_worker_exit(worker_generation);
}
