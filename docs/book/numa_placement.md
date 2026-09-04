# NUMA-Aware Placement

Mnemosyne uses Themis for NUMA identity and topology, and owns the allocator
operations that consume that information. Themis provides `NumaNodeId`,
`PlacementHint`, and the cached/current-node queries. Mnemosyne provides
per-node segment pools, segment binding, interleaved allocation, and
first-touch support.

There are two intentionally separate integration layers:

- `mnemosyne-arena` keeps the segment allocator hot path small. Its public
  node queries expose the numeric node value required by the segment metadata,
  and its pool routes retained segments by that value.
- `mnemosyne-heap` re-exports Themis's typed `NumaNodeId` and
  `PlacementHint`. Its `TieredHeap` façade resolves a hint to a memory tier
  and applies the node-level binding for host allocations.

## Querying the Caller's Node

`mnemosyne-arena::numa` wraps Themis's thread-local node query:

```rust,ignore
use mnemosyne_arena::current_numa_node;
use mnemosyne_arena::numa::refresh_numa_node;

let node: u32 = current_numa_node(); // cached query
let node: u32 = refresh_numa_node(); // refreshes from the OS
```

The `u32` return is deliberate at this arena boundary: segment metadata and
the fixed bucket table use the numeric node value. Callers that need the
typed placement vocabulary use `themis::current_numa_node()` directly or the
`mnemosyne-heap` API.

Every newly mapped segment records the current numeric node in
`Segment::numa_node` before publication. A segment recycled from a pool keeps
its originating node and is not rebound merely because a later caller runs on
another node. This preserves the pool's locality contract; remote reuse is an
explicit fallback when local capacity is unavailable.

## Per-Node Segment Pools

`GlobalSegmentPool` is an array of 16 node buckets rather than one contended
list. `numa_bucket::bucket_index` maps arbitrary node identifiers into that
fixed table through Themis's
`NumaNodeId::bucket_index::<NUMA_BUCKETS>()`. The table is an implementation
bound, so machines with more than 16 node identifiers fold them into the
available buckets.

`GlobalSegmentPool::pop` is local-first:

1. Pop from the bucket for the caller's cached node.
2. After a bounded sequence of misses, refresh the cached node and retry if
   the thread migrated.
3. Walk the remaining buckets in wrap order and steal the first available
   segment.

The refresh is rate-limited to avoid an OS query on every miss. A remote
segment is graceful degradation, not an allocation failure. The same bucket
partitioning is used by the retained huge-allocation pool, which additionally
indexes by allocation-size bands.

## Explicit Binding and First Touch

`mnemosyne-heap::numa` owns the public raw-allocation primitives. The arena's
`bind_segment_to_numa_node` is a separate internal segment-path primitive: it
has no error return and treats a failed Linux policy call as best effort.

| Primitive | Linux | Windows | Other |
|-----------|-------|---------|-------|
| `bind_to_node` | `mbind(MPOL_BIND)`; returns a typed error on policy failure | documented no-op that returns `Ok(())` | documented no-op that returns `Ok(())` |
| `allocate_interleaved` | standard allocation followed by best-effort `mbind(MPOL_INTERLEAVE)` | `VirtualAllocExNuma` with per-node chunked commits when multiple nodes are reported | plain allocation |
| `first_touch` | volatile write at a 4 KiB stride | same | same |

`bind_to_node` returns `NumaError::InvalidNode` when the Linux nodemask
cannot represent the requested node and `NumaError::Syscall` when the kernel
rejects `mbind`. `TieredHeap::alloc` deliberately consumes that result: the
block remains usable when a placement hint cannot be honored. A caller that
needs to observe the policy result calls `bind_to_node` directly and handles
the `Result`.

`allocate_interleaved` reports host-allocation failure. Its policy operation is
best effort after the allocation succeeds, so a policy failure does not leak
or invalidate the returned block. The returned pointer must still be released
with the matching backend deallocator.

## Tiered Heap Routing

`TieredHeap::alloc` accepts Themis's `PlacementHint` and performs both halves
of the route:

1. `tier_for` resolves `PlacementHint::Numa(node)` to host `MemoryTier::Dram`.
2. The host sub-heap allocates the block.
3. The façade calls `bind_to_node` for the requested node.

The node hint therefore selects host locality; it does not select device,
HBM, GDDR, or host-pinned memory. Those choices use
`PlacementHint::Tier(...)`. The `Registers` and `SharedMem` tiers are
budget-only GPU capacity and return `None` from the allocator rather than
fabricating an address-space allocation.

## First-Touch Policy

On systems using first-touch placement, mapping memory does not by itself
choose the final physical page location. The first write faults each page on
the accessing thread's node. `first_touch` makes that placement event
explicit, using a volatile write at a 4 KiB stride so every larger host page
receives at least one access.

Linux `bind_to_node` adds an `MPOL_BIND` policy to the range, so placement is
the combination of the requested policy and page faults. Mnemosyne applies
that policy to a fresh arena segment before publishing it. Pooled segments
retain their prior binding; rebinding every reuse would add kernel traffic to
the allocator hot path and would contradict the segment pool's originating
node ownership. Use explicit binding or first-touch only at a deliberate
placement boundary, not inside per-element allocation loops.
