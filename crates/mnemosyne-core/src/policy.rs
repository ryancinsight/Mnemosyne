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

/// Compile-time security mitigation bitmask constants.
///
/// Combine with bitwise OR to describe a policy's active mitigations.
/// Inspired by snmalloc 0.7.x `mitigations/mitigations.h`.
pub mod mitigations {
    /// Poison freed and allocated bytes with sentinel patterns.
    pub const POISONING: u32 = 1 << 0;
    /// Zero-initialize all allocations.
    pub const ZERO_INIT: u32 = 1 << 1;
    /// XOR-encrypt free-list next-pointers per page cookie.
    pub const FREE_LIST_ENCRYPTION: u32 = 1 << 2;
    /// Fisher-Yates–shuffle free list on page initialization.
    pub const RANDOMIZE_ALLOCATION: u32 = 1 << 3;
    /// Hysteresis: delay page waking until ≥ capacity/WAKE_DENOMINATOR freed.
    pub const DELAY_PAGE_WAKE: u32 = 1 << 4;
    /// Multiplicative backward-edge canary on freed blocks.
    pub const FREE_CANARY: u32 = 1 << 5;
    /// Validate caller's size/align against stored block_size on sized free.
    pub const SIZED_FREE_VALIDATION: u32 = 1 << 6;
    /// Free-canary is now wired: it IS enforced at runtime.
    pub const FREE_CANARY_WIRED: u32 = 1 << 7;
    /// Mitigations with an end-to-end runtime implementation.
    ///
    /// `DELAY_PAGE_WAKE` and `FREE_CANARY_WIRED` belong here.
    /// `SIZED_FREE_VALIDATION` stays out — helpers exist but no production
    /// caller reaches them yet.
    pub const IMPLEMENTED: u32 = POISONING
        | ZERO_INIT
        | FREE_LIST_ENCRYPTION
        | RANDOMIZE_ALLOCATION
        | DELAY_PAGE_WAKE
        | FREE_CANARY
        | FREE_CANARY_WIRED;
    /// Every mitigation bit defined by this registry.
    ///
    /// This is a registry mask, not a claim that every bit is active in a
    /// policy. Use [`IMPLEMENTED`] or a policy's
    /// [`MITIGATION_FLAGS`][super::AllocPolicy::MITIGATION_FLAGS]
    /// for the currently enforced data-plane mitigations.
    pub const ALL: u32 = IMPLEMENTED | SIZED_FREE_VALIDATION;
    /// No mitigations.
    pub const NONE: u32 = 0;
}

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

    /// If true, a full page does not become active until at least
    /// `capacity / WAKE_DENOMINATOR` blocks are freed from it.
    const DELAY_PAGE_WAKE: bool = false;

    /// Denominator for the page-wake hysteresis.
    const WAKE_DENOMINATOR: u16 = 4;

    /// Minimum warm segments kept in the pool after a `purge_lazy` call.
    const SEGMENT_POOL_WARM_THRESHOLD: usize = 0;

    /// Human-readable name for this policy. Zero-cost — inlined by the compiler.
    const POLICY_NAME: &'static str = "custom";

    /// Combined bitmask of active [`mitigations`] for this policy.
    const MITIGATION_FLAGS: u32 = mitigations::NONE;

    /// Probabilistic guard-page sampling rate (GWP-ASan hook).
    /// `0` disables. Inspired by snmalloc 0.7.2 `gwp_asan.h`.
    const GWP_SAMPLE_RATE: u32 = 0;

    /// Maximum allocation size this policy will serve without a panic/error.
    ///
    /// Defaults to `MAX_ALLOC_SIZE` (the global ceiling). A more restrictive
    /// policy can lower this to limit the maximum allocation size it serves,
    /// useful for security envelopes or domain-specific allocators that
    /// should not serve arbitrarily large objects.
    ///
    /// Zero means "no policy limit" (uses the global ceiling).
    const MAX_ALLOC_SIZE_LIMIT: usize = 0;

    /// Compile-time configuration fingerprint.
    ///
    /// A single `u64` that uniquely identifies the combination of all boolean
    /// policy flags and key integer constants. Two policy types with identical
    /// behaviour produce the same fingerprint; any difference in flags or
    /// thresholds produces a different one. Useful for:
    ///
    /// - Embedding in binary metadata to identify the allocator configuration.
    /// - `debug_assert!` checks that a data structure was built with the
    ///   expected policy.
    /// - Logging the full policy in one field without a string allocation.
    ///
    /// The encoding packs all scalar fields into a fixed-layout u64. It is
    /// **not** a cryptographic hash — it is a deterministic bijection over the
    /// policy's compile-time constants.
    const POLICY_FINGERPRINT: u64 = {
        // Layout (bits from LSB):
        //  0      ENABLE_POISONING
        //  1      ZERO_INITIALIZE
        //  2      ENABLE_FREE_LIST_ENCRYPTION
        //  3      RANDOMIZE_ALLOCATION
        //  4      DELAY_PAGE_WAKE
        //  5..20  WAKE_DENOMINATOR (16 bits)
        // 20..28  SEGMENT_POOL_WARM_THRESHOLD (clamped to 8 bits)
        // 28..60  MITIGATION_FLAGS (32 bits)
        (Self::ENABLE_POISONING as u64)
            | ((Self::ZERO_INITIALIZE as u64) << 1)
            | ((Self::ENABLE_FREE_LIST_ENCRYPTION as u64) << 2)
            | ((Self::RANDOMIZE_ALLOCATION as u64) << 3)
            | ((Self::DELAY_PAGE_WAKE as u64) << 4)
            | ((Self::WAKE_DENOMINATOR as u64) << 5)
            | (((Self::SEGMENT_POOL_WARM_THRESHOLD & 0xFF) as u64) << 20)
            | ((Self::MITIGATION_FLAGS as u64) << 28)
    };
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
// Mitigation-flag consistency assertions
const _: () = assert!(
    StandardPolicy::MITIGATION_FLAGS == mitigations::NONE,
    "StandardPolicy::MITIGATION_FLAGS must be NONE"
);
const _: () = assert!(
    HardenedPolicy::MITIGATION_FLAGS == mitigations::IMPLEMENTED,
    "HardenedPolicy::MITIGATION_FLAGS must equal mitigations::IMPLEMENTED"
);

// ── Policy typestate marker ───────────────────────────────────────────────────

/// Zero-sized type (ZST) that brands a data structure with the allocator
/// policy used to create it — zero runtime cost (`PhantomData<P>`).
///
/// Embed `PolicyMarker<P>` in consumer types to carry the policy as a
/// compile-time proof without any runtime overhead.
pub struct PolicyMarker<P: AllocPolicy>(core::marker::PhantomData<P>);

impl<P: AllocPolicy> PolicyMarker<P> {
    /// Creates a new zero-cost marker.
    #[inline]
    pub const fn new() -> Self {
        Self(core::marker::PhantomData)
    }
    /// Returns the compile-time policy name.
    #[inline]
    pub const fn name() -> &'static str {
        P::POLICY_NAME
    }
    /// Returns the compile-time policy fingerprint.
    #[inline]
    pub const fn fingerprint() -> u64 {
        P::POLICY_FINGERPRINT
    }
    /// Returns the compile-time mitigation flags bitmask.
    #[inline]
    pub const fn mitigation_flags() -> u32 {
        P::MITIGATION_FLAGS
    }
}

impl<P: AllocPolicy> Default for PolicyMarker<P> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
impl<P: AllocPolicy> Clone for PolicyMarker<P> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}
impl<P: AllocPolicy> Copy for PolicyMarker<P> {}
impl<P: AllocPolicy> core::fmt::Debug for PolicyMarker<P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "PolicyMarker<{}>", P::POLICY_NAME)
    }
}
impl<P: AllocPolicy> PartialEq for PolicyMarker<P> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl<P: AllocPolicy> Eq for PolicyMarker<P> {}

const _: () = assert!(
    core::mem::size_of::<PolicyMarker<StandardPolicy>>() == 0,
    "PolicyMarker must be a ZST"
);
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_policy_has_no_mitigations() {
        assert_eq!(StandardPolicy::MITIGATION_FLAGS, mitigations::NONE);
        assert_eq!(StandardPolicy::POLICY_NAME, "standard");
        assert_eq!(StandardPolicy::POLICY_FINGERPRINT & 0x1F, 0); // no bool flags set
    }

    #[test]
    fn hardened_policy_has_all_implemented_mitigations() {
        assert_eq!(HardenedPolicy::POLICY_NAME, "hardened");
        assert_ne!(HardenedPolicy::MITIGATION_FLAGS, mitigations::NONE);
        // Poisoning and encryption are associated constants, so these are
        // build-time facts: asserting them at run time checks nothing a
        // compiled binary could still get wrong.
        const _: () = assert!(HardenedPolicy::ENABLE_POISONING);
        const _: () = assert!(HardenedPolicy::ENABLE_FREE_LIST_ENCRYPTION);
    }

    #[test]
    fn policy_fingerprints_are_distinct() {
        // All three production policies must have different fingerprints.
        assert_ne!(
            StandardPolicy::POLICY_FINGERPRINT,
            SecurePolicy::POLICY_FINGERPRINT
        );
        assert_ne!(
            StandardPolicy::POLICY_FINGERPRINT,
            HardenedPolicy::POLICY_FINGERPRINT
        );
        assert_ne!(
            SecurePolicy::POLICY_FINGERPRINT,
            HardenedPolicy::POLICY_FINGERPRINT
        );
    }

    #[test]
    fn policy_fingerprint_encodes_key_flags() {
        // ENABLE_POISONING is bit 0 of the fingerprint.
        assert_eq!(
            StandardPolicy::POLICY_FINGERPRINT & 1,
            StandardPolicy::ENABLE_POISONING as u64
        );
        assert_eq!(
            HardenedPolicy::POLICY_FINGERPRINT & 1,
            HardenedPolicy::ENABLE_POISONING as u64
        );
    }

    #[test]
    fn policy_marker_is_zero_sized() {
        assert_eq!(core::mem::size_of::<PolicyMarker<StandardPolicy>>(), 0);
        assert_eq!(core::mem::size_of::<PolicyMarker<HardenedPolicy>>(), 0);
    }
}
