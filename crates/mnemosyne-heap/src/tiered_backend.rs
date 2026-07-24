//! Tier-aware backend vocabulary façade.
//!
//! [`TieredBackend`] maps every `MemoryTier` to one of the five concrete
//! `MemoryBackend` impls that participate in [`crate::tiered_heap::TieredHeap`]:
//! host (`MemoryBackendWrapper`), CUDA page-locked host
//! (`CudaHostPinnedBackend`), and tier-keyed CUDA device-local
//! (`CudaDeviceBackend`, `CudaHbmBackend`, `CudaGddrBackend`).
//!
//! It is **not itself** a `MemoryBackend` impl. The
//! [`mnemosyne_core::MemoryBackend`] trait is *not* object-safe
//! (associated constants and associated functions without `&self` make
//! `dyn MemoryBackend` a type error, at the trait-style "static dispatch
//! vocabulary" this crate uses). The façade instead returns a small
//! `Copy` enum ([`TierSelection`]) that callers match against. Typed
//! dispatch into the underlying backends lives in
//! [`crate::tiered_heap::TieredHeap`] which monomorphizes one
//! [`crate::heap::Heap`] per [`TierSelection`] variant.
//!
//! This shape has two properties worth flagging:
//!
//! 1. **No thread-local state.** A `MemoryBackend` impl backed by
//!    implicit `thread_local!` would route allocations using a tier
//!    setting leaked across boundaries (a panic / re-entrant allocator
//!    call would misclassify the tier and corrupt the wrong pool). The
//!    typed enum keeps the tier knowledge *with the value* (the
//!    [`crate::tiered_heap::TieredBlock`] carries it) instead of
//!    stashing it in TLS.
//! 2. **Single dispatch table.** Both [`TieredBackend`] and
//!    [`crate::tiered_heap::TieredHeap`] call into
//!    [`TierSelection`] for per-tier routing — the host / host-pinned /
//!    device classification lives in exactly one place
//!    ([`TieredBackend::for_tier`]), eliminating the duplication between
//!    the two leaf modules.
//!
//! # Atlas ADR 0002 alignment
//!
//! Budget-only tiers [`MemoryTier::Registers`] and [`MemoryTier::SharedMem`]
//! return `None` from `for_tier` and `false` from `supports`. They
//! represent GPU compiler-assigned / launch-declared capacity (not
//! address space) and stay paired with
//! [`mnemosyne_core::KernelResourceBudget`] in moirai-gpu's occupancy
//! planner and hephaestus-{wgpu,cuda} launch planning.

use crate::tier::MemoryTier;

/// Backend-keyed selection produced by [`TieredBackend::for_tier`].
///
/// `Copy` because each variant is a tag against the five concrete
/// backends in [`mnemosyne_backend`]; runtime payloads are zero-sized
/// unit types so the enum is also `Eq`/`Hash`/etc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TierSelection {
    /// Host memory: standard `Dram` or persistent.
    Host,
    /// CUDA page-locked host (DMA-staging).
    HostPinned,
    /// CUDA device-local memory with unspecified technology.
    Device,
    /// CUDA device-local memory with the HBM pool identity.
    Hbm,
    /// CUDA device-local memory with the GDDR pool identity.
    Gddr,
}

/// Tier-aware backend vocabulary façade.
///
/// Zero-sized type. Holding this in a struct field or generic parameter
/// is a no-op; the value is purely a documented hook for "this code
/// routes tier-aware allocations".
pub struct TieredBackend;

impl TieredBackend {
    /// Returns the [`TierSelection`] for a [`MemoryTier`], or `None`
    /// for the budget-only tiers `Registers` and `SharedMem`.
    ///
    /// The dispatch table:
    ///
    /// | `MemoryTier`       | `TierSelection` |
    /// |--------------------|------------------|
    /// | `Dram`             | `Host`           |
    /// | `Hbm`              | `Hbm`            |
    /// | `Persistent`       | `Host`           |
    /// | `HostPinned`       | `HostPinned`     |
    /// | `Device`           | `Device`         |
    /// | `Gddr`             | `Gddr`           |
    /// | `Registers`        | `None`           |
    /// | `SharedMem`        | `None`           |
    #[inline]
    #[must_use = "dropping the resolved tier-backend mapping discards the dispatch result"]
    pub fn for_tier(tier: MemoryTier) -> Option<TierSelection> {
        match tier {
            MemoryTier::Dram | MemoryTier::Persistent => Some(TierSelection::Host),
            MemoryTier::Hbm => Some(TierSelection::Hbm),
            MemoryTier::HostPinned => Some(TierSelection::HostPinned),
            MemoryTier::Device => Some(TierSelection::Device),
            MemoryTier::Gddr => Some(TierSelection::Gddr),
            MemoryTier::Registers | MemoryTier::SharedMem => None,
        }
    }

    /// Returns `true` iff `tier` is host-allocatable (i.e. has a
    /// concrete backend in [`TieredBackend::for_tier`]).
    ///
    /// Const-friendly thin re-export of [`MemoryTier::is_host_allocatable`]
    /// so callers can write `TieredBackend::supports(tier)` at the heap
    /// façade boundary instead of reaching into themis directly.
    #[inline]
    #[must_use]
    pub const fn supports(tier: MemoryTier) -> bool {
        tier.is_host_allocatable()
    }
}
