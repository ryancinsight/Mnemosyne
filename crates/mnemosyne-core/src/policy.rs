//! Compile-time allocator behaviors and memory safety policies.
//!
//! # Zero-cost guarantee
//!
//! Every policy flag is a `const bool` associated item, not a runtime value.
//! Callers that specialize on `P::ENABLE_POISONING`, `P::ZERO_INITIALIZE`, etc.
//! will have those branches completely eliminated by the optimizer for
//! `StandardPolicy` (all false), leaving no overhead versus a hand-coded
//! non-poisoning allocator.  The compile-time assertions at the bottom of this
//! module verify this invariant at build time.

#[doc(hidden)]
pub mod private {
    pub trait Sealed {}
}

/// A sealed trait representing an allocator behavior and safety policy.
///
/// # Design: Zero-Sized Types + Const Booleans
///
/// Each policy is a ZST (`size_of::<P>() == 0`); the flags are `const bool`
/// associated items that the compiler evaluates at monomorphization time.
/// Branches on `P::ENABLE_POISONING` in hot paths are unconditionally dead for
/// `StandardPolicy` and unconditionally live for `HardenedPolicy`; no runtime
/// conditional is emitted.
pub trait AllocPolicy: private::Sealed + Send + Sync + 'static {
    /// If true, write poison bytes to memory on allocation and deallocation to detect heap corruption.
    const ENABLE_POISONING: bool;

    /// If true, zero-initialize all memory allocations.
    const ZERO_INITIALIZE: bool;

    /// Byte pattern to write into memory when it is freed.
    const POISON_FREE_BYTE: u8 = 0xDE;

    /// Byte pattern to write into memory when it is allocated.
    const POISON_ALLOC_BYTE: u8 = 0xAD;

    /// If true, encrypt free list next pointers using the per-segment XOR key.
    const ENABLE_FREE_LIST_ENCRYPTION: bool = false;

    /// If true, randomize the allocation order of blocks in a page.
    const RANDOMIZE_ALLOCATION: bool = false;
}

/// Zero-Sized Type (ZST) representing the standard allocation policy with maximum performance.
///
/// All flags are `false`; every policy-guarded branch is dead code and is
/// eliminated at compile time. `StandardPolicy` allocations and frees pay no
/// poisoning or zeroing cost.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StandardPolicy;

impl private::Sealed for StandardPolicy {}
impl AllocPolicy for StandardPolicy {
    const ENABLE_POISONING: bool = false;
    const ZERO_INITIALIZE: bool = false;
}

/// Zero-Sized Type (ZST) representing a secure allocation policy with memory
/// poisoning and zero-initialization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SecurePolicy;

impl private::Sealed for SecurePolicy {}
impl AllocPolicy for SecurePolicy {
    const ENABLE_POISONING: bool = true;
    const ZERO_INITIALIZE: bool = true;
    const RANDOMIZE_ALLOCATION: bool = true;
}

/// Zero-Sized Type (ZST) representing a hardened allocation policy with memory
/// poisoning, zero-initialization, and free-list XOR encryption.
///
/// The freelist encryption uses a triangular XOR key:
/// `page_address ^ per_thread_seed ^ process_key`, where both seeds come from
/// OS entropy via `std::collections::hash_map::RandomState`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HardenedPolicy;

impl private::Sealed for HardenedPolicy {}
impl AllocPolicy for HardenedPolicy {
    const ENABLE_POISONING: bool = true;
    const ZERO_INITIALIZE: bool = true;
    const ENABLE_FREE_LIST_ENCRYPTION: bool = true;
    const RANDOMIZE_ALLOCATION: bool = true;
}

/// Compile-time assertions that `StandardPolicy` carries no non-ZST overhead.
///
/// These `const _: ()` blocks evaluate during compilation; a policy that
/// accidentally sets `ENABLE_POISONING = true` in `StandardPolicy` would fail
/// to build rather than silently incur a performance regression.
const _: () = assert!(
    !StandardPolicy::ENABLE_POISONING,
    "StandardPolicy must have ENABLE_POISONING = false (zero-cost guarantee)"
);
const _: () = assert!(
    !StandardPolicy::ZERO_INITIALIZE,
    "StandardPolicy must have ZERO_INITIALIZE = false (zero-cost guarantee)"
);
const _: () = assert!(
    !StandardPolicy::ENABLE_FREE_LIST_ENCRYPTION,
    "StandardPolicy must have ENABLE_FREE_LIST_ENCRYPTION = false (zero-cost guarantee)"
);
const _: () = assert!(
    core::mem::size_of::<StandardPolicy>() == 0,
    "StandardPolicy must be a ZST"
);
const _: () = assert!(
    core::mem::size_of::<SecurePolicy>() == 0,
    "SecurePolicy must be a ZST"
);
const _: () = assert!(
    core::mem::size_of::<HardenedPolicy>() == 0,
    "HardenedPolicy must be a ZST"
);
