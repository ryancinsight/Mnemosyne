//! The [`AllocPolicy`] sealed trait.
//!
//! All policy flags are `const` associated items evaluated at monomorphization
//! time; every flag-guarded branch is dead code for policies that disable it.

use super::mitigations;
use super::private;

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
    /// If true, write poison bytes to memory on allocation and deallocation to
    /// detect heap corruption.
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

    /// Combined bitmask of active [`mitigations`][super::mitigations] for this
    /// policy.
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
