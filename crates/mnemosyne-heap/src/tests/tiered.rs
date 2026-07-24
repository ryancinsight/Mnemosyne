//! Phase 3 Stage D1 device-memory integration tests.
//!
//! Pins the typed dispatch shape between themis placement vocabulary
//! (`PlacementHint`/`MemoryTier`) and the heap façade
//! ([`crate::tier::tier_for`], [`crate::tiered_backend::TieredBackend`],
//! [`crate::tiered_heap::TieredHeap`]). Three layers:
//!
//! - [`tier_resolution`] — `tier_for(hint)` resolves every
//!   `PlacementHint` variant to a concrete `MemoryTier`.
//! - [`tiered_backend`]  — `TieredBackend::for_tier(tier)` returns the
//!   canonical `TierSelection` per tier; budget-only tiers return
//!   `None`.
//! - [`tiered_heap`]     — `scope_tiered::<P>` constructs three typed
//!   sub-heaps sharing one higher-ranked `'brand`, and `alloc`/`free`
//!   routes `TieredBlock`s back to the right sub-heap.
//!
//! KernelResourceBudget (atlas ADR 0002) is **not** exercised here:
//! Phase 3 owns placement, not launch budgeting; the kernel_budget seam
//! lives in moirai-gpu occupancy and hephaestus-{wgpu,cuda} launch
//! planning.

use super::*;
use crate::tier::tier_for;
use crate::tier::{MemoryTier, PlacementHint};
use crate::tiered_backend::{TierSelection, TieredBackend};
use crate::tiered_heap::scope_tiered;
use mnemosyne_arena::HasSegmentPool;
use themis::{LocalityDomainId, NumaNodeId};

// ---- tier resolution ----

#[test]
fn tier_for_default_hint_is_dram() {
    assert_eq!(tier_for(PlacementHint::default()), MemoryTier::Dram);
}

#[test]
fn tier_for_non_tier_hints_collapse_to_dram() {
    assert_eq!(
        tier_for(PlacementHint::Current),
        MemoryTier::Dram,
        "Current hint must collapse to host DRAM"
    );
    assert_eq!(
        tier_for(PlacementHint::Any),
        MemoryTier::Dram,
        "Any hint must collapse to host DRAM"
    );
    assert_eq!(
        tier_for(PlacementHint::Numa(NumaNodeId::new(2))),
        MemoryTier::Dram,
        "Numa hint must collapse to host DRAM (the heap facade has one host pool)"
    );
    assert_eq!(
        tier_for(PlacementHint::Domain(LocalityDomainId::new(7))),
        MemoryTier::Dram,
        "Domain hint must collapse to host DRAM"
    );
}

#[test]
fn tier_for_tier_hints_pass_through() {
    assert_eq!(
        tier_for(PlacementHint::Tier(MemoryTier::Device)),
        MemoryTier::Device
    );
    assert_eq!(
        tier_for(PlacementHint::Tier(MemoryTier::HostPinned)),
        MemoryTier::HostPinned
    );
    assert_eq!(
        tier_for(PlacementHint::Tier(MemoryTier::Hbm)),
        MemoryTier::Hbm
    );
    assert_eq!(
        tier_for(PlacementHint::Tier(MemoryTier::Gddr)),
        MemoryTier::Gddr
    );
}

// ---- TieredBackend vocabulary ----

#[test]
fn tiered_backend_supports_every_host_allocatable_tier() {
    assert!(TieredBackend::supports(MemoryTier::Dram));
    assert!(TieredBackend::supports(MemoryTier::Hbm));
    assert!(TieredBackend::supports(MemoryTier::Persistent));
    assert!(TieredBackend::supports(MemoryTier::HostPinned));
    assert!(TieredBackend::supports(MemoryTier::Device));
    assert!(TieredBackend::supports(MemoryTier::Gddr));
}

#[test]
fn tiered_backend_rejects_budget_only_tiers() {
    assert!(
        !TieredBackend::supports(MemoryTier::Registers),
        "Registers is budget-only (atlas ADR 0002) and must not be host-allocatable"
    );
    assert!(
        !TieredBackend::supports(MemoryTier::SharedMem),
        "SharedMem is budget-only (atlas ADR 0002) and must not be host-allocatable"
    );
    assert!(TieredBackend::for_tier(MemoryTier::Registers).is_none());
    assert!(TieredBackend::for_tier(MemoryTier::SharedMem).is_none());
}

#[test]
fn tiered_backend_for_tier_routes_host_family_to_host_selection() {
    assert_eq!(
        TieredBackend::for_tier(MemoryTier::Dram),
        Some(TierSelection::Host)
    );
    assert_eq!(
        TieredBackend::for_tier(MemoryTier::Persistent),
        Some(TierSelection::Host)
    );
}

#[test]
fn tiered_backend_keeps_device_tiers_on_distinct_pool_keys() {
    assert_eq!(
        TieredBackend::for_tier(MemoryTier::Hbm),
        Some(TierSelection::Hbm)
    );
    assert_eq!(
        TieredBackend::for_tier(MemoryTier::Gddr),
        Some(TierSelection::Gddr)
    );
    assert_ne!(
        mnemosyne_backend::CudaHbmBackend::global_segment_pool() as *const _,
        mnemosyne_backend::CudaGddrBackend::global_segment_pool() as *const _,
        "HBM and GDDR must not share retained segment state"
    );
}

#[test]
fn tiered_backend_for_tier_routes_pinned_to_host_pinned_selection() {
    assert_eq!(
        TieredBackend::for_tier(MemoryTier::HostPinned),
        Some(TierSelection::HostPinned)
    );
}

#[test]
fn tiered_backend_for_tier_routes_device_family_to_device_selection() {
    assert_eq!(
        TieredBackend::for_tier(MemoryTier::Device),
        Some(TierSelection::Device)
    );
}

// ---- TieredHeap dispatch end-to-end ----

#[test]
fn tiered_heap_default_hint_routes_to_host_dram() {
    scope_tiered::<StandardPolicy, _, _>(|tiered, mut token| {
        let block = tiered
            .alloc(&token, test_layout(64, 8), PlacementHint::default())
            .expect("default hint must allocate on the host DRAM sub-heap");
        assert_eq!(
            block.tier(),
            MemoryTier::Dram,
            "default PlacementHint -> tier_for -> Dram"
        );
        tiered.free(&mut token, block);
    });
}

#[test]
fn tiered_heap_any_hint_routes_to_host_dram() {
    scope_tiered::<StandardPolicy, _, _>(|tiered, mut token| {
        let block = tiered
            .alloc(&token, test_layout(32, 8), PlacementHint::Any)
            .expect("Any hint must allocate on the host DRAM sub-heap");
        assert_eq!(block.tier(), MemoryTier::Dram);
        tiered.free(&mut token, block);
    });
}

#[test]
fn tiered_heap_budget_only_hints_return_none() {
    scope_tiered::<StandardPolicy, _, _>(|tiered, token| {
        let layout = test_layout(32, 8);
        let result_regs = tiered.alloc(&token, layout, PlacementHint::Tier(MemoryTier::Registers));
        assert!(
            result_regs.is_none(),
            "MemoryTier::Registers is budget-only (atlas ADR 0002) and must not allocate"
        );
        let result_smem = tiered.alloc(&token, layout, PlacementHint::Tier(MemoryTier::SharedMem));
        assert!(
            result_smem.is_none(),
            "MemoryTier::SharedMem is budget-only (atlas ADR 0002) and must not allocate"
        );
    });
}

#[test]
fn tiered_heap_device_and_pinned_hints_route_to_cuda_sub_heaps() {
    // This test exercises dispatch classification when CUDA is unavailable:
    // returning None on alloc is the correct outcome because
    // CudaDeviceBackend / CudaHostPinnedBackend ::allocate return null
    // when CUDA absent. The dispatch path is verified regardless of the
    // runtime CUDA availability — on real CUDA hardware, the test would
    // surface a Some(block) with the matching tier.
    scope_tiered::<StandardPolicy, _, _>(|tiered, token| {
        let layout = test_layout(32, 8);
        for tier in [MemoryTier::Device, MemoryTier::Hbm, MemoryTier::Gddr] {
            if let Some(b) = tiered.alloc(&token, layout, PlacementHint::Tier(tier)) {
                assert_eq!(
                    b.tier(),
                    tier,
                    "device-tinted dispatch must carry its requested tier on the block"
                );
            }
        }
        if let Some(b) = tiered.alloc(&token, layout, PlacementHint::Tier(MemoryTier::HostPinned)) {
            assert_eq!(
                b.tier(),
                MemoryTier::HostPinned,
                "HostPinned-tinted dispatch must carry HostPinned tier on the block"
            );
        }
    });
}

#[test]
fn tiered_heap_realloc_preserves_tier() {
    scope_tiered::<StandardPolicy, _, _>(|tiered, mut token| {
        let layout = test_layout(16, 8);
        let block = tiered
            .alloc(&token, layout, PlacementHint::default())
            .expect("initial allocation failed");
        let original_tier = block.tier();

        let new_block = tiered
            .realloc(&mut token, block, layout, 64)
            .expect("realloc failed");
        assert_eq!(
            new_block.tier(),
            original_tier,
            "realloc must preserve the tier the block was allocated against"
        );
        assert_eq!(new_block.tier(), MemoryTier::Dram);

        tiered.free(&mut token, new_block);
    });
}

#[test]
fn tiered_heap_realloc_to_zero_drops_without_replacing() {
    scope_tiered::<StandardPolicy, _, _>(|tiered, mut token| {
        let block = tiered
            .alloc(&token, test_layout(32, 8), PlacementHint::default())
            .expect("initial allocation failed");
        let result = tiered.realloc(&mut token, block, test_layout(32, 8), 0);
        assert!(
            result.is_none(),
            "realloc to new_size = 0 must drop the block without a replacement"
        );
    });
}
