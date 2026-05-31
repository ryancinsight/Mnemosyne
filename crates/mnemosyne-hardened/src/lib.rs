#![no_std]

use mnemosyne_core::policy::{AllocPolicy, private::Sealed};

/// Zero-Sized Type (ZST) representing a secure allocation policy with memory poisoning and zero-initialization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SecurePolicy;

impl Sealed for SecurePolicy {}

impl AllocPolicy for SecurePolicy {
    const ENABLE_POISONING: bool = true;
    const ZERO_INITIALIZE: bool = true;
    const RANDOMIZE_ALLOCATION: bool = true;
}

/// Zero-Sized Type (ZST) representing a hardened allocation policy with memory poisoning, zero-initialization, and free-list encryption.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HardenedPolicy;

impl Sealed for HardenedPolicy {}

impl AllocPolicy for HardenedPolicy {
    const ENABLE_POISONING: bool = true;
    const ZERO_INITIALIZE: bool = true;
    const ENABLE_FREE_LIST_ENCRYPTION: bool = true;
    const RANDOMIZE_ALLOCATION: bool = true;
}
