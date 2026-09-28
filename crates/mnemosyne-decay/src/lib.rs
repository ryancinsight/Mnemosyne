//! Background decay and reclamation for Mnemosyne arenas.
//!
//! Segments freed by the allocator are not returned to the operating system
//! immediately: holding them lets a subsequent allocation of the same size
//! class reuse the mapping without a syscall. This crate owns the opposite
//! side of that trade — it periodically purges segments that have gone cold,
//! so a burst of allocation does not pin resident memory indefinitely.
//!
//! # Module organisation
//!
//! | Module | Responsibility |
//! |--------|---------------|
//! | [`control`] | Public API: spawn, trigger, observe, step |
//! | [`engine`] | Background thread loop and adaptive interval |
//! | [`events`] | Condvar-based synchronisation (step/wake signals) |
//! | [`orphan`] | Per-backend orphan-pool draining |
//! | [`state`] | Process-global counters and event cells |
//!
//! [`init_decay_engine`] lazily spawns the worker thread. [`decay_step`]
//! performs one sweep and is public so a caller with its own scheduler can
//! drive reclamation without the background thread. [`request_decay_step`]
//! wakes an already-running worker immediately.

#![deny(missing_docs)]

mod control;
mod engine;
mod events;
mod orphan;
mod state;

pub use control::{
    decay_step, decay_step_generation, init_decay_engine, request_decay_step,
    wait_for_decay_shutdown, wait_for_decay_step,
};
