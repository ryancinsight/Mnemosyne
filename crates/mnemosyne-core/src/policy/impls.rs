//! Concrete ZST policy implementations.
//!
//! Each type is a Zero-Sized Type (ZST) — `size_of::<StandardPolicy>() == 0` —
//! with all flags as `const bool` items that the compiler resolves at
//! monomorphization. Compile-time assertions below verify the zero-cost
//! contract for [`StandardPolicy`].

use super::alloc_policy::AllocPolicy;
use super::mitigations;
use super::private;

/// Zero-Sized Type (ZST) representing the standard allocation policy with
/// maximum performance.
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
    /// Keep 4 committed free segments warm for rapid free-then-allocate bursts.
    const SEGMENT_POOL_WARM_THRESHOLD: usize = 4;
    const POLICY_NAME: &'static str = "standard";
    const MITIGATION_FLAGS: u32 = mitigations::NONE;
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
    const POLICY_NAME: &'static str = "secure";
    const MITIGATION_FLAGS: u32 =
        mitigations::POISONING | mitigations::ZERO_INIT | mitigations::RANDOMIZE_ALLOCATION;
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
    /// Page-waking hysteresis: a full page does not re-enter the active list
    /// until at least `capacity / WAKE_DENOMINATOR` blocks have been freed.
    /// This widens the temporal window between free and realloc, making
    /// use-after-free and LIFO heap-spray exploits harder to land.
    /// Zero-cost in `StandardPolicy` (branch eliminated at monomorphization).
    const DELAY_PAGE_WAKE: bool = true;
    const POLICY_NAME: &'static str = "hardened";
    /// All currently implemented mitigations active.
    const MITIGATION_FLAGS: u32 = mitigations::IMPLEMENTED;
}

// ── Compile-time zero-cost assertions ─────────────────────────────────────────
//
// These `const _: ()` blocks evaluate during compilation; a policy that
// accidentally sets `ENABLE_POISONING = true` in `StandardPolicy` would fail
// to build rather than silently incur a performance regression.

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
const _: () = assert!(
    StandardPolicy::MITIGATION_FLAGS == mitigations::NONE,
    "StandardPolicy::MITIGATION_FLAGS must be NONE"
);
const _: () = assert!(
    HardenedPolicy::MITIGATION_FLAGS == mitigations::IMPLEMENTED,
    "HardenedPolicy::MITIGATION_FLAGS must equal mitigations::IMPLEMENTED"
);
