//! Atomic primitives, swappable for loom's model-checking versions.
//!
//! Under `cfg(loom)` this re-exports `loom::sync::atomic`; otherwise
//! `core::sync::atomic`. Concurrency-bearing types import atomics from here so
//! a loom model can drive the *same* code the allocator ships, rather than a
//! transcription of it that is free to drift.
//!
//! # Why a cfg and not a feature
//!
//! Loom replaces the atomic types with instrumented ones that only work inside
//! `loom::model`. A cargo feature is additive and unifies across a build, so a
//! feature would risk linking loom atomics into a normal build. `cfg(loom)` is
//! set only by `RUSTFLAGS="--cfg loom"` for the dedicated model runs, which
//! cannot leak into an ordinary `cargo build`.
//!
//! # Scope
//!
//! Only the types the modelled structures need are re-exported. Adding one is
//! deliberate: every type here widens what a loom model must account for.

#[cfg(loom)]
pub use loom::sync::atomic::{AtomicPtr, AtomicUsize, Ordering, fence};

#[cfg(not(loom))]
pub use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering, fence};
