# NUMA-Aware Allocation

Non-uniform memory access means *where* memory lives can be as important as
*how much* is allocated.  Mnemosyne exposes the current node and provides a
tiered placement surface so hot data can live close to the cores that touch
it, and device memory can be requested explicitly.

## Current node

```rust
use mnemosyne_arena::current_numa_node;

let node = current_numa_node();
```

`current_numa_node() -> u32` reads the calling thread's node affinity, with
`refresh_numa_node()` available when the caller has reason to believe the
affinity changed (for example after `sched_setaffinity`).  Segment
allocation uses the node to decide where to map, and huge-block retention is
tracked per NUMA node (`retained_huge_blocks`/`retained_huge_bytes` in
`MemoryStats`).

## Memory tiers

Placement vocabulary is owned by **themis** and re-exported through
`mnemosyne-heap`:

```rust
pub use themis::{MemoryTier, PlacementHint}; // in mnemosyne-heap
```

`MemoryTier` distinguishes host memory classes:

- `Host` — ordinary system RAM.
- `HostPinned` — page-locked host staging memory, exposed as an independent
  staging pool for copy engines.
- `Hbm` / `Gddr` — device-adjacent technologies (high-bandwidth memory /
  GDDR).  These have independent zero-sized CUDA backend identities and
  retained segment/TLS pools.  The shared CUDA driver remains the allocation
  provider because its managed-memory API does not select a physical
  technology, so **no physical HBM/GDDR placement guarantee is claimed**.

## Tiered heap

The tiered surface lives in `mnemosyne-heap` (the `mnemosyne` facade
re-exports the branded core — `Heap`, `ThreadLocalToken`, `branded_scope` —
but the tiered entry points are used from the heap crate):

```rust
use mnemosyne_heap::{
    PlacementHint, ThreadLocalToken, TieredBlock, scope_tiered,
};

scope_tiered(|heap, token| {
    let layout = std::alloc::Layout::array::<u8>(1024).unwrap();
    let block = heap.alloc(&token, layout, PlacementHint::HostPinned);
    // block: Option<TieredBlock<'_, u8>> — dropped back into its tier's pool
});
```

`TieredHeap::alloc(&self, token: &ThreadLocalToken<'brand>, layout: Layout,
hint: PlacementHint) -> Option<TieredBlock<'brand, u8>>` routes the
allocation to the pool for the requested hint's tier, backed by
`TieredBackend` (a `MemoryBackend` that dispatches by tier).  The `token`
is the same `ThreadLocalToken<'brand>` the scoped entry point hands to the
closure, tying the block's lifetime to the scope.  `TieredReallocError`
mirrors the branded `ReallocError`: a failed tiered realloc returns the
source block so it cannot leak.  The `Hbm` and `Gddr` tiers each own their
retained segment/TLS pools so a tier never borrows another tier's free
lists.

## Placement hints

`PlacementHint` is the caller-facing request.  Hints are compile-time typed
through the tier machinery, so a `PlacementHint::HostPinned` allocation is
statically different from a `PlacementHint::Host` one — the tiered heap
cannot silently serve one from the other's pool.
