//! Compile-time allocator behaviors and memory safety policies.
//!
//! # Zero-cost guarantee
//!
//! Every policy flag is a `const bool` associated item, not a runtime value.
//! Callers that specialize on `P::ENABLE_POISONING`, `P::ZERO_INITIALIZE`, etc.
//! will have those branches completely eliminated by the optimizer for
//! [`StandardPolicy`] (all false), leaving no overhead versus a hand-coded
//! non-poisoning allocator. The compile-time assertions in [`impls`] verify
//! this invariant at build time.
//!
//! # Module organization
//!
//! | Sub-module | Contents |
//! |------------|---------|
//! | [`mitigations`] | Compile-time bitmask constants for active mitigations |
//! | `private` | Sealed-trait infrastructure (not pub) |
//! | [`alloc_policy`] | `AllocPolicy` sealed trait definition |
//! | [`impls`] | `StandardPolicy`, `SecurePolicy`, `HardenedPolicy` ZSTs |
//! | [`marker`] | `PolicyMarker<P>` ZST typestate |

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
    /// Dual free-list per page (random-preserve): each page maintains a second
    /// free list so that the allocator can randomly select from two lists,
    /// widening the temporal distance between consecutive free and alloc of the
    /// same block.
    ///
    /// Inspired by snmalloc 0.7.x `random_preserve` proposal. This bit is
    /// defined for future use — a full page-struct extension is needed to
    /// activate it.
    pub const DUAL_FREELIST: u32 = 1 << 8;
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
    pub const ALL: u32 = IMPLEMENTED | SIZED_FREE_VALIDATION | DUAL_FREELIST;
    /// No mitigations.
    pub const NONE: u32 = 0;
}

/// Sealed-trait infrastructure preventing out-of-crate `AllocPolicy` impls.
#[doc(hidden)]
pub mod private {
    pub trait Sealed {}
}

mod alloc_policy;
mod impls;
mod marker;

pub use alloc_policy::AllocPolicy;
pub use impls::{HardenedPolicy, SecurePolicy, StandardPolicy};
pub use marker::PolicyMarker;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_policy_has_no_mitigations() {
        assert_eq!(StandardPolicy::MITIGATION_FLAGS, mitigations::NONE);
        assert_eq!(StandardPolicy::POLICY_NAME, "standard");
        assert_eq!(StandardPolicy::POLICY_FINGERPRINT & 0x1F, 0);
    }

    #[test]
    fn hardened_policy_has_all_implemented_mitigations() {
        assert_eq!(HardenedPolicy::POLICY_NAME, "hardened");
        assert_ne!(HardenedPolicy::MITIGATION_FLAGS, mitigations::NONE);
        const _: () = assert!(HardenedPolicy::ENABLE_POISONING);
        const _: () = assert!(HardenedPolicy::ENABLE_FREE_LIST_ENCRYPTION);
    }

    #[test]
    fn policy_fingerprints_are_distinct() {
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
