//! Tier-aware heap façade on top of the internal raw heap.
//!
//! [`TieredHeap`] owns five typed [`crate::heap::Heap`] instances,
//! each a thin wrapper over one raw-heap monomorphization:
//!
//! | `TierSelection`     | Sub-heap backend                              |
//! |---------------------|-----------------------------------------------|
//! | `Host`              | [`mnemosyne_backend::MemoryBackendWrapper`]   |
//! | `HostPinned`        | [`mnemosyne_backend::CudaHostPinnedBackend`]  |
//! | `Device`             | [`mnemosyne_backend::CudaDeviceBackend`]      |
//! | `Hbm`               | [`mnemosyne_backend::CudaHbmBackend`]         |
//! | `Gddr`              | [`mnemosyne_backend::CudaGddrBackend`]       |
//!
//! All five sub-heaps share one higher-ranked `'brand` lifetime minted
//! by [`scope_tiered`] via [`melinoe::sync::thread_local_scope`]; each
//! sub-heap's `BrandedBlock` carries the same brand so [`TieredHeap::free`]
//! can route a `TieredBlock` back to the right pool using its carried
//! `MemoryTier`.
//!
//! # Atlas ADR 0002 alignment
//!
//! Budget-only tiers `Registers` / `SharedMem` never pass through this
//! façade — they are kernel-side capacity vocabulary consumed by
//! [`mnemosyne_core::KernelResourceBudget`] (moirai-gpu occupancy and
//! hephaestus-{wgpu,cuda} launch planning) and not address space that
//! the allocator owns. [`TieredHeap::alloc`] returns `None` for those
//! tiers instead of fabricating a host/cheap fallback, so a misuse
//! surfaces at the call site rather than silently allocating in the
//! wrong pool.

use crate::brand::{BrandedBlock, ThreadLocalToken};
use crate::heap::Heap;
use crate::raw_heap::RawHeap;
use crate::tier::{MemoryTier, PlacementHint, tier_for};
use crate::tiered_backend::{TierSelection, TieredBackend};
use core::alloc::Layout;
use core::marker::PhantomData;
use melinoe::sync::thread_local_scope;
use mnemosyne_core::AllocPolicy;

/// A tier-aware heap façade.
///
/// Holds five typed [`Heap`] sub-heaps under one higher-ranked `'brand`
/// lifetime. [`alloc`](Self::alloc) routes a `PlacementHint` to the
/// right sub-heap and wraps the resulting `BrandedBlock` in a
/// [`TieredBlock`] carrying the resolved tier; [`free`](Self::free) and
/// [`realloc`](Self::realloc) use the carried tier to route the block
/// back to the right sub-heap.
///
/// Sub-heap backends are fixed at the type level (no runtime selection):
/// the dispatch table lives in [`TieredBackend::for_tier`]; this
/// façade just matches against the resulting [`TierSelection`].
pub struct TieredHeap<'brand, P: AllocPolicy> {
    host: Heap<'brand, P, mnemosyne_backend::MemoryBackendWrapper>,
    device: Heap<'brand, P, mnemosyne_backend::CudaDeviceBackend>,
    hbm: Heap<'brand, P, mnemosyne_backend::CudaHbmBackend>,
    gddr: Heap<'brand, P, mnemosyne_backend::CudaGddrBackend>,
    pinned: Heap<'brand, P, mnemosyne_backend::CudaHostPinnedBackend>,
}

// SAFETY: `TieredHeap<'brand, P>` is `Send` because each sub-heap
// `Heap<'brand, P, B>` is `Send` per the existing
// `unsafe impl<'brand, P, B: HasSegmentPool> Send for Heap<'brand, P, B>`
// (`MemoryBackendWrapper`, `CudaDeviceBackend`, `CudaHostPinnedBackend`
// each implement `HasSegmentPool` in `mnemosyne-arena`); the sub-heaps'
// `Send` derives from the underlying `RawHeap`'s
// `unsafe impl<P, B: HasSegmentPool> Send for RawHeap<P, B>`. The
// `'brand` lifetime is captured by the sub-heaps' `_phantom` fields —
// no extra `PhantomData` is needed on this struct since `'brand` only
// appears transitively through the sub-heap types.
//
// The `'brand` invariant lifetime also enforces thread-locality at the
// API surface: the only way to mint a `'brand` is through
// [`scope_tiered`] / [`melinoe::sync::thread_local_scope`], which keeps
// the `ThreadLocalToken` (and therefore the heap) on the spawning
// thread. `unsafe impl Send` is the necessary trait surface so the
// heap can move between threads in pathological call patterns; the
// scope mint is what actually prevents cross-thread sharing at runtime.
unsafe impl<'brand, P: AllocPolicy> Send for TieredHeap<'brand, P> {}

/// A [`BrandedBlock`] enriched with the [`MemoryTier`] it was allocated
/// against.
///
/// The tier field is the only place the tier lives once an allocation
/// has happened — the underlying `BrandedBlock` does not know its pool,
/// and the heap façade needs the tier at `free` time to return the
/// memory to the right sub-heap.
pub struct TieredBlock<'brand, T: ?Sized> {
    pub(crate) block: BrandedBlock<'brand, T>,
    pub(crate) tier: MemoryTier,
}

impl<'brand, T: ?Sized> TieredBlock<'brand, T> {
    /// Returns the [`MemoryTier`] this block was allocated against.
    ///
    /// Same value as [`crate::tier::tier_for`] applied to the original
    /// `PlacementHint`; carried alongside the block so cross-tier free
    /// can route back to the right pool without re-passing the hint.
    #[inline(always)]
    #[must_use]
    pub fn tier(&self) -> MemoryTier {
        self.tier
    }

    /// Returns the raw pointer to the block's managed memory.
    ///
    /// Convenience accessor matching [`BrandedBlock::as_ptr`].
    #[inline(always)]
    #[must_use]
    pub fn as_ptr(&self) -> *mut T {
        self.block.as_ptr()
    }
}

impl<'brand, P: AllocPolicy> TieredHeap<'brand, P> {
    /// Allocates a block of memory and routes it to the right sub-heap
    /// based on `hint`.
    ///
    /// Returns `Some(TieredBlock)` if the resolved tier is host-allocatable
    /// *and* the sub-heap's `alloc` succeeded. Returns `None` for:
    /// - The budget-only tiers `Registers` / `SharedMem` (atlas
    ///   ADR 0002: GPU compiler-assigned / launch-declared capacity,
    ///   not address space), or
    /// - A sub-heap allocation failure (out of host memory; CUDA out
    ///   of pinned / device memory; etc.).
    ///
    /// The returned `TieredBlock::tier()` always matches
    /// [`crate::tier::tier_for`] applied to `hint`.
    #[inline]
    #[must_use = "dropping the result discards the allocation outcome"]
    pub fn alloc(
        &self,
        token: &ThreadLocalToken<'brand>,
        layout: Layout,
        hint: PlacementHint,
    ) -> Option<TieredBlock<'brand, u8>> {
        let tier = tier_for(hint);
        match TieredBackend::for_tier(tier) {
            Some(TierSelection::Host) => self
                .host
                .alloc(token, layout)
                .map(|b| TieredBlock { block: b, tier }),
            Some(TierSelection::HostPinned) => self
                .pinned
                .alloc(token, layout)
                .map(|b| TieredBlock { block: b, tier }),
            Some(TierSelection::Device) => self
                .device
                .alloc(token, layout)
                .map(|b| TieredBlock { block: b, tier }),
            Some(TierSelection::Hbm) => self
                .hbm
                .alloc(token, layout)
                .map(|b| TieredBlock { block: b, tier }),
            Some(TierSelection::Gddr) => self
                .gddr
                .alloc(token, layout)
                .map(|b| TieredBlock { block: b, tier }),
            // None covers the budget-only tiers (`Registers`,
            // `SharedMem`): atlas ADR 0002 budgets that the GPU
            // compiler and kernel launch own, not address space the
            // allocator owns. Pairing with launch planning stays in
            // moirai-gpu occupancy and hephaestus-{wgpu,cuda} via
            // `mnemosyne_core::KernelResourceBudget`.
            None => {
                debug_assert!(
                    matches!(tier, MemoryTier::Registers | MemoryTier::SharedMem),
                    "non-budget-only `MemoryTier` reached the budget-only reject arm — dispatch-table drift between `tier_for` and `TieredBackend::for_tier`"
                );
                None
            }
        }
    }

    /// Frees a `TieredBlock` back to the sub-heap it was allocated from.
    ///
    /// Routed by `block.tier()`. Budget-only blocks cannot be allocated
    /// by [`Self::alloc`] and so cannot reach this method, but the
    /// unallocated match arm is preserved as `unreachable!()` style
    /// empty body for type-system completeness.
    #[inline]
    pub fn free<T: ?Sized>(
        &self,
        token: &mut ThreadLocalToken<'brand>,
        block: TieredBlock<'brand, T>,
    ) {
        let tier = block.tier;
        let inner = block.block;
        match TieredBackend::for_tier(tier) {
            Some(TierSelection::Host) => self.host.free(token, inner),
            Some(TierSelection::HostPinned) => self.pinned.free(token, inner),
            Some(TierSelection::Device) => self.device.free(token, inner),
            Some(TierSelection::Hbm) => self.hbm.free(token, inner),
            Some(TierSelection::Gddr) => self.gddr.free(token, inner),
            // A `TieredBlock` carrying a budget-only tier cannot
            // exist via the safe `alloc` path; reaching this arm
            // means a non-alloc construction spliced a tier that
            // `TieredBackend::for_tier` rejects. Leaking is no worse
            // than the underlying unsoundness; the debug assertion
            // catches the regression under `cargo test`.
            None => debug_assert!(
                matches!(tier, MemoryTier::Registers | MemoryTier::SharedMem),
                "TieredBlock of a tier that `TieredBackend::for_tier` rejects leaked through a non-alloc construction path"
            ),
        }
    }

    /// Reallocates a `TieredBlock` in place on its sub-heap, carrying
    /// the same tier over to the new block.
    ///
    /// Returns `None` if `new_size == 0` (block is dropped per
    /// `Heap::realloc`'s zero-realloc contract) or if the underlying
    /// sub-heap realloc fails. On success the returned block's tier
    /// matches the input.
    #[inline]
    #[must_use = "dropping the result discards the realloc outcome"]
    pub fn realloc<T: ?Sized>(
        &self,
        token: &mut ThreadLocalToken<'brand>,
        block: TieredBlock<'brand, T>,
        layout: Layout,
        new_size: usize,
    ) -> Option<TieredBlock<'brand, u8>> {
        let tier = block.tier;
        let inner = block.block;
        let new_block = match TieredBackend::for_tier(tier) {
            Some(TierSelection::Host) => self.host.realloc(token, inner, layout, new_size),
            Some(TierSelection::HostPinned) => self.pinned.realloc(token, inner, layout, new_size),
            Some(TierSelection::Device) => self.device.realloc(token, inner, layout, new_size),
            Some(TierSelection::Hbm) => self.hbm.realloc(token, inner, layout, new_size),
            Some(TierSelection::Gddr) => self.gddr.realloc(token, inner, layout, new_size),
            // See `alloc` / `free` rationale: a budget-only-tier
            // `TieredBlock` cannot reach this path through a safe
            // construction. Debug-assert the invariant; the empty body
            // is the type-system completion for the budget-only arm.
            None => {
                debug_assert!(
                    matches!(tier, MemoryTier::Registers | MemoryTier::SharedMem),
                    "TieredBlock of a budget-only tier reached realloc — non-alloc construction path"
                );
                let _ = layout;
                let _ = new_size;
                None
            }
        };
        new_block.map(|b| TieredBlock { block: b, tier })
    }
}

/// Mints one compile-time unique brand and constructs a [`TieredHeap`].
///
/// Mirrors [`crate::brand::scope`] but constructs five typed sub-heaps
/// sharing the same brand, then hands both to the closure. The
/// higher-ranked `'brand` keeps the heap and the token capability
/// provably paired for the scope's lifetime; they cannot escape the
/// closure because the melinoe [`ThreadLocalToken`] is `!Send + !Sync`
/// and the `'brand` lifetime is higher-ranked and scoped.
///
/// # Examples
///
/// ```
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_heap::{tiered_heap::scope_tiered, tier::{MemoryTier, PlacementHint}};
/// use core::alloc::Layout;
///
/// scope_tiered::<StandardPolicy, _, _>(|tiered, mut token| {
///     let block = tiered
///         .alloc(&token, Layout::from_size_align(64, 8).unwrap(), PlacementHint::default())
///         .expect("default hint must route to host DRAM");
///     assert_eq!(block.tier(), MemoryTier::Dram);
///     tiered.free(&mut token, block);
/// });
/// ```
pub fn scope_tiered<P: AllocPolicy, F, R>(f: F) -> R
where
    F: for<'brand> FnOnce(TieredHeap<'brand, P>, ThreadLocalToken<'brand>) -> R,
{
    thread_local_scope(|token| {
        let heap: TieredHeap<'_, P> = TieredHeap {
            host: Heap {
                raw: RawHeap::<P, mnemosyne_backend::MemoryBackendWrapper>::new(),
                _phantom: PhantomData,
            },
            device: Heap {
                raw: RawHeap::<P, mnemosyne_backend::CudaDeviceBackend>::new(),
                _phantom: PhantomData,
            },
            hbm: Heap {
                raw: RawHeap::<P, mnemosyne_backend::CudaHbmBackend>::new(),
                _phantom: PhantomData,
            },
            gddr: Heap {
                raw: RawHeap::<P, mnemosyne_backend::CudaGddrBackend>::new(),
                _phantom: PhantomData,
            },
            pinned: Heap {
                raw: RawHeap::<P, mnemosyne_backend::CudaHostPinnedBackend>::new(),
                _phantom: PhantomData,
            },
        };
        f(heap, token)
    })
}
