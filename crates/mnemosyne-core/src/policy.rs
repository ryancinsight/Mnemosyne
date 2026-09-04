//! Compile-time allocator behaviors and memory safety policies.

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
}

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

