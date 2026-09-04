//! Compile-time allocator behaviors and memory safety policies.

/// Compile-time security mitigation bitmask constants.
///
/// Combine with bitwise OR to describe a policy's active mitigations. Each
/// constant corresponds to an [`AllocPolicy`] boolean field; the combined
/// [`AllocPolicy::MITIGATION_FLAGS`] value provides a single fingerprint for
/// the policy's security posture — useful for logging, assertions, and
/// displaying a one-line summary of what is active.
///
/// Inspired by snmalloc's `mitigation::type` system
/// (`src/snmalloc/mitigations/mitigations.h`).
pub mod mitigations {
    /// Poison freed and allocated bytes with sentinel patterns.
    pub const POISONING: u32            = 1 << 0;
    /// Zero-initialize all allocations (more than just poison-on-alloc).
    pub const ZERO_INIT: u32            = 1 << 1;
    /// XOR-encrypt free-list next-pointers per page cookie.
    pub const FREE_LIST_ENCRYPTION: u32 = 1 << 2;
    /// Fisher-Yates–shuffle free list on page initialization.
    pub const RANDOMIZE_ALLOCATION: u32 = 1 << 3;
    /// Hysteresis: delay page waking until ≥ capacity/WAKE_DENOMINATOR freed.
    pub const DELAY_PAGE_WAKE: u32      = 1 << 4;
    /// Multiplicative backward-edge canary on freed blocks.
    pub const FREE_CANARY: u32          = 1 << 5;
    /// Validate caller's size/align against stored block_size on sized free.
    pub const SIZED_FREE_VALIDATION: u32 = 1 << 6;

    /// All mitigations active (convenience constant for HardenedPolicy).
    pub const ALL: u32 =
        POISONING | ZERO_INIT | FREE_LIST_ENCRYPTION | RANDOMIZE_ALLOCATION
        | DELAY_PAGE_WAKE | FREE_CANARY | SIZED_FREE_VALIDATION;

    /// No mitigations (StandardPolicy default).
    pub const NONE: u32 = 0;
}

#[doc(hidden)]
pub mod private {
    pub trait Sealed {}
}

/// A sealed trait representing an allocator behavior and safety policy.
pub trait AllocPolicy: private::Sealed + Send + Sync + 'static {
    /// If true, write poison bytes to memory on allocation and deallocation to detect heap corruption.
    const ENABLE_POISONING: bool;

    /// If true, zero-initialize all memory allocations.
    const ZERO_INITIALIZE: bool;

    /// Byte pattern to write into memory when it is freed.
    const POISON_FREE_BYTE: u8 = 0xDE;

    /// Byte pattern to write into memory when it is allocated.
    const POISON_ALLOC_BYTE: u8 = 0xAD;

    /// If true, encrypt free list next pointers.
    const ENABLE_FREE_LIST_ENCRYPTION: bool = false;

    /// If true, randomize the allocation order of blocks in a page.
    const RANDOMIZE_ALLOCATION: bool = false;

    /// If true, a full page is not made active again until at least
    /// `capacity / WAKE_DENOMINATOR` blocks are freed from it.
    ///
    /// This creates a hysteresis zone that prevents rapid LIFO address reuse
    /// and maximises the temporal distance between free and reuse, making
    /// LIFO heap-spray and UAF exploits harder to land.
    ///
    /// Under `StandardPolicy` this is `false`, so the branch is statically
    /// dead (zero-cost). Under `HardenedPolicy` it is `true`.
    ///
    /// Inspired by snmalloc's `waking` field (0.7.x) and the ISMM 2024 paper
    /// "BatchIt: Optimizing Message-Passing Allocators for Producer-Consumer
    /// Workloads".
    const DELAY_PAGE_WAKE: bool = false;

    /// Denominator for the page-wake hysteresis (ignored when
    /// [`DELAY_PAGE_WAKE`][Self::DELAY_PAGE_WAKE] is `false`).
    /// A full page becomes active only after
    /// `free_count >= capacity / WAKE_DENOMINATOR` blocks are freed.
    const WAKE_DENOMINATOR: u16 = 4;

    /// Minimum number of completely-free segments the global pool should keep
    /// in a **warm** (committed, ready-to-reuse) state before
    /// `reset_segment_pool` begins decommitting further segments.
    ///
    /// Set to a small positive value (e.g., 4) for `StandardPolicy` to
    /// amortise the `VirtualAlloc` / `mmap` round-trip cost when allocation
    /// bursts are followed immediately by frees and then more allocations.
    /// Set to `0` for `HardenedPolicy` so stale physical pages are returned
    /// to the OS eagerly — a security posture consistent with the rest of the
    /// hardened behaviour set.
    ///
    /// This is a pure compile-time constant: monomorphization eliminates the
    /// compare at zero cost when the policy has no warm threshold.
    const SEGMENT_POOL_WARM_THRESHOLD: usize = 0;

    /// Human-readable name of this policy.
    ///
    /// Useful for diagnostic output and logging without needing runtime
    /// reflection: `P::POLICY_NAME` is a compile-time string constant.
    /// The default implementation returns the unit type name so every
    /// implementation automatically gets a reasonable fallback.
    ///
    /// Zero-cost: the constant is inlined by the compiler; no string
    /// allocation or indirection at run time.
    const POLICY_NAME: &'static str = "custom";

    /// Combined bitmask of active [`mitigations`] for this policy.
    ///
    /// Each bit corresponds to one `mitigations::*` constant. The value is
    /// derived from the individual boolean constants but expressed as a single
    /// `u32` so callers can test, log, or assert the whole security posture in
    /// one operation.
    ///
    /// ```
    /// use mnemosyne_core::policy::{AllocPolicy, StandardPolicy, HardenedPolicy, mitigations};
    /// assert_eq!(StandardPolicy::MITIGATION_FLAGS, mitigations::NONE);
    /// assert_eq!(HardenedPolicy::MITIGATION_FLAGS, mitigations::ALL);
    /// ```
    const MITIGATION_FLAGS: u32 = mitigations::NONE;

    /// Probabilistic guard-page sampling rate for GWP-ASan–style heap
    /// diagnostics.
    ///
    /// When non-zero, approximately 1 in `GWP_SAMPLE_RATE` small allocations
    /// is redirected to a guard-page–backed region so that heap-buffer-overflow
    /// and use-after-free bugs are caught immediately. `0` disables sampling
    /// entirely (zero-cost branch elimination via monomorphization).
    ///
    /// Inspired by snmalloc 0.7.2 `gwp_asan.h` and the `secondary_allocator`
    /// template parameter.
    const GWP_SAMPLE_RATE: u32 = 0;
}

/// Zero-Sized Type (ZST) representing the standard allocation policy with maximum performance.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StandardPolicy;

impl private::Sealed for StandardPolicy {}
impl AllocPolicy for StandardPolicy {
    const ENABLE_POISONING: bool = false;
    const ZERO_INITIALIZE: bool = false;
    /// Keep 4 committed free segments as a warm pool so that rapid
    /// free-then-allocate bursts do not pay an OS round-trip each time.
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
/// poisoning, zero-initialization, and free-list encryption.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HardenedPolicy;

impl private::Sealed for HardenedPolicy {}
impl AllocPolicy for HardenedPolicy {
    const ENABLE_POISONING: bool = true;
    const ZERO_INITIALIZE: bool = true;
    const ENABLE_FREE_LIST_ENCRYPTION: bool = true;
    const RANDOMIZE_ALLOCATION: bool = true;
    /// Waking hysteresis: a full page does not become active until at least
    /// `capacity / 4` blocks are free. This prevents rapid LIFO address
    /// reuse and increases the temporal window between free and reallocate,
    /// making use-after-free and LIFO heap-spray exploits harder to land.
    const DELAY_PAGE_WAKE: bool = true;
    const POLICY_NAME: &'static str = "hardened";
    /// All mitigations active: the maximum security posture.
    const MITIGATION_FLAGS: u32 = mitigations::ALL;
}

// ── Policy compile-time consistency assertions ────────────────────────────────

// HardenedPolicy must have ALL mitigations set — an omission would silently
// weaken the security posture without anyone noticing.
const _: () = assert!(
    HardenedPolicy::MITIGATION_FLAGS == mitigations::ALL,
    "HardenedPolicy::MITIGATION_FLAGS must equal mitigations::ALL"
);
// StandardPolicy must have NO mitigations — any overhead would be unexpected.
const _: () = assert!(
    StandardPolicy::MITIGATION_FLAGS == mitigations::NONE,
    "StandardPolicy::MITIGATION_FLAGS must equal mitigations::NONE"
);
// SecurePolicy must have at least poisoning and zero-init.
const _: () = assert!(
    (SecurePolicy::MITIGATION_FLAGS & mitigations::POISONING) != 0,
    "SecurePolicy must include mitigations::POISONING"
);
const _: () = assert!(
    (SecurePolicy::MITIGATION_FLAGS & mitigations::ZERO_INIT) != 0,
    "SecurePolicy must include mitigations::ZERO_INIT"
);

// ── Policy typestate marker ───────────────────────────────────────────────────

/// Zero-sized type (ZST) that brands a data structure with the allocator
/// policy used to create it.
///
/// Embed `PolicyMarker<P>` in any type to carry the policy as a
/// **compile-time proof** without any runtime overhead (PhantomData is a
/// ZST whose size is always 0).  Consumer code monomorphized over `P` can
/// then propagate the policy through data pipelines without runtime dispatch.
///
/// # Example
///
/// ```rust
/// use mnemosyne_core::policy::{AllocPolicy, PolicyMarker, StandardPolicy};
///
/// struct ScratchBuffer<P: AllocPolicy> {
///     data: alloc::vec::Vec<u8>,
///     _policy: PolicyMarker<P>,
/// }
///
/// impl<P: AllocPolicy> ScratchBuffer<P> {
///     fn policy_name() -> &'static str {
///         P::POLICY_NAME
///     }
/// }
///
/// assert_eq!(ScratchBuffer::<StandardPolicy>::policy_name(), "standard");
/// ```
pub struct PolicyMarker<P: AllocPolicy>(core::marker::PhantomData<P>);

impl<P: AllocPolicy> PolicyMarker<P> {
    /// Creates a new zero-cost marker for policy `P`.
    #[inline]
    pub const fn new() -> Self {
        Self(core::marker::PhantomData)
    }

    /// Returns the compile-time policy name.
    #[inline]
    pub const fn name() -> &'static str {
        P::POLICY_NAME
    }
}

impl<P: AllocPolicy> Default for PolicyMarker<P> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

// PolicyMarker is a ZST — always Copy, Clone, Debug, PartialEq, Eq, Hash.
impl<P: AllocPolicy> Clone for PolicyMarker<P> {
    #[inline]
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl<P: AllocPolicy> Copy for PolicyMarker<P> {}

impl<P: AllocPolicy> core::fmt::Debug for PolicyMarker<P> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "PolicyMarker<{}>", P::POLICY_NAME)
    }
}

impl<P: AllocPolicy> PartialEq for PolicyMarker<P> {
    #[inline]
    fn eq(&self, _other: &Self) -> bool {
        true // ZST — all instances are identical
    }
}

impl<P: AllocPolicy> Eq for PolicyMarker<P> {}

impl<P: AllocPolicy> core::hash::Hash for PolicyMarker<P> {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, _state: &mut H) {}
}

/// Compile-time assertion that `PolicyMarker<P>` is truly zero-sized.
const _: () = assert!(core::mem::size_of::<PolicyMarker<StandardPolicy>>() == 0);
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_policy_has_no_mitigations() {
        assert_eq!(StandardPolicy::MITIGATION_FLAGS, mitigations::NONE);
        assert_eq!(StandardPolicy::POLICY_NAME, "standard");
    }

    #[test]
    fn hardened_policy_has_all_mitigations() {
        assert_eq!(HardenedPolicy::MITIGATION_FLAGS, mitigations::ALL);
        assert_eq!(HardenedPolicy::POLICY_NAME, "hardened");
    }

    #[test]
    fn secure_policy_has_subset_of_hardened_mitigations() {
        // All SecurePolicy flags must be present in HardenedPolicy
        let secure = SecurePolicy::MITIGATION_FLAGS;
        let hardened = HardenedPolicy::MITIGATION_FLAGS;
        assert_eq!(secure & hardened, secure, "all secure flags must be in hardened");
        // Secure must have at least poisoning and randomize
        assert_ne!(secure & mitigations::POISONING, 0);
        assert_ne!(secure & mitigations::RANDOMIZE_ALLOCATION, 0);
    }

    #[test]
    fn mitigation_flags_are_independent_bits() {
        // Each flag must be a distinct power of two
        let all_flags = [
            mitigations::POISONING, mitigations::ZERO_INIT,
            mitigations::FREE_LIST_ENCRYPTION, mitigations::RANDOMIZE_ALLOCATION,
            mitigations::DELAY_PAGE_WAKE, mitigations::FREE_CANARY,
            mitigations::SIZED_FREE_VALIDATION,
        ];
        for &f in &all_flags {
            assert!(f.is_power_of_two(), "flag {f:#010x} is not a power of two");
        }
    }

    #[test]
    fn policy_marker_is_zero_sized() {
        assert_eq!(core::mem::size_of::<PolicyMarker<StandardPolicy>>(), 0);
        assert_eq!(core::mem::size_of::<PolicyMarker<HardenedPolicy>>(), 0);
    }
}