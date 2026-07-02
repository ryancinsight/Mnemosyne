#![no_std]

//! Security-hardened allocation policies.
//!
//! The `SecurePolicy` and `HardenedPolicy` ZSTs are defined in
//! [`mnemosyne_core::policy`] alongside `StandardPolicy`, the single
//! authoritative home for `AllocPolicy` implementations. This crate re-exports
//! them under its historical name so existing dependents
//! (`use mnemosyne_hardened::{SecurePolicy, HardenedPolicy}`, and downstream
//! Cargo manifests that list `mnemosyne-hardened`) resolve unchanged. It holds
//! no logic of its own.

pub use mnemosyne_core::policy::{HardenedPolicy, SecurePolicy};
