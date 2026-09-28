//! Shared test scaffolding for this crate's unit tests.
//!
//! The `record_*` telemetry in [`crate::recorders`] is process-global, so every
//! unit test that drives or snapshots it must run under one lock. This was
//! previously three separate `static TEST_LOCK` definitions across
//! [`crate::guard`], [`crate::reset`], and [`crate::recorders::stats`], with two
//! different acquire idioms; a single definition here is the one serialization
//! point for the whole crate.

extern crate std;

use std::sync::{Mutex, MutexGuard};

/// Serializes the crate's telemetry unit tests against the global counters.
static TEST_LOCK: Mutex<()> = Mutex::new(());

/// Acquires [`TEST_LOCK`], recovering from a poisoned mutex.
///
/// The lock guards only global counters, so a poisoned mutex carries no
/// invariants the next test needs: recovering through `into_inner` keeps one
/// failing test from cascading into spurious failures in every test that runs
/// afterwards.
pub(crate) fn lock_test() -> MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
