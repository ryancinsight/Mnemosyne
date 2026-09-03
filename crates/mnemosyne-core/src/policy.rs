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
}

/// Zero-Sized Type (ZST) representing the standard allocation policy with maximum performance.
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
}
