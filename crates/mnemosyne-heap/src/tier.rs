//! Tier-aware placement vocabulary for the heap façade.
//!
//! Re-exports `themis::law::{MemoryTier, PlacementHint}` so heap callers
//! can refer to a single canonical name (`mnemosyne_heap::tier::{MemoryTier,
//! PlacementHint}`) and provides [`tier_for`] to resolve any
//! `PlacementHint` to a concrete `MemoryTier`. The resolver is the join
//! point between caller-supplied hints (NUMA, domain, current, any, tier)
//! and the tier-keyed dispatch in [`crate::tiered_backend::TieredBackend`]
//! and [`crate::tiered_heap::TieredHeap`].
//!
//! # Atlas ADR 0002 alignment
//!
//! The host-allocatable / budget-only distinction is owned by
//! [`MemoryTier::is_host_allocatable`]; `Registers` and `SharedMem` return
//! `false` because GPU compilers assign registers and kernels declare
//! shared memory at launch, so they are capacity vocabulary for
//! [`mnemosyne_core::KernelResourceBudget`] occupancy planning — never an
//! allocator request. The heap's [`crate::tiered_heap::TieredHeap::alloc`]
//! reflects that by returning `None` for budget-only tiers.
//!
//! # Examples
//!
//! ```
//! use mnemosyne_heap::tier::{tier_for, MemoryTier, PlacementHint};
//! assert_eq!(tier_for(PlacementHint::default()), MemoryTier::Dram);
//! assert_eq!(tier_for(PlacementHint::Any), MemoryTier::Dram);
//! assert_eq!(
//!     tier_for(PlacementHint::Tier(MemoryTier::Device)),
//!     MemoryTier::Device,
//! );
//! assert_eq!(
//!     tier_for(PlacementHint::Tier(MemoryTier::HostPinned)),
//!     MemoryTier::HostPinned,
//! );
//! ```

pub use themis::{MemoryTier, PlacementHint};

/// Resolves a `PlacementHint` to the concrete `MemoryTier` that the heap
/// façade will dispatch against.
///
/// Non-tier hints (`Current`, `Any`, `Numa`, `Domain`) collapse to
/// [`MemoryTier::Dram`] — the host block pool — because the heap façade
/// has a single host `RawHeap` instance and that pool lives on standard
/// host DRAM. `Tier(t)` passes the tier through unchanged so the
/// per-tier dispatch in [`crate::tiered_heap::TieredHeap::alloc`] and
/// [`crate::tiered_backend::TieredBackend::for_tier`] can route to the
/// right sub-heap or concrete backend, including the budget-only tiers
/// `Registers`/`SharedMem` that the façade will reject.
///
/// `#[inline(always)]` is justified: this resolver sits on the alloc
/// hot-path inside `scope_tiered` and is a small match with no branching /
/// memory side-effects; inlining keeps the call site branch-minimal.
#[inline(always)]
pub const fn tier_for(hint: PlacementHint) -> MemoryTier {
    match hint {
        PlacementHint::Tier(t) => t,
        PlacementHint::Current
        | PlacementHint::Any
        | PlacementHint::Numa(_)
        | PlacementHint::Domain(_) => MemoryTier::Dram,
    }
}
