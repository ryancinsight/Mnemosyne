# Position in the Stack

Mnemosyne is Atlas's memory substrate. It owns allocation, reclamation,
arena lifetime, host-side staging storage, and memory instrumentation. It is
not the owner of numerical meaning, execution scheduling, placement policy,
GPU resource protocols, or scientific file formats.

## What Mnemosyne owns

The public `mnemosyne-memory` package exposes the Rust library name
`mnemosyne`. Its explicit allocator and arena APIs are backed by the
workspace's focused crates:

- `mnemosyne-core` owns allocation constants, size-class mapping, validation,
  policy contracts, and representation-level laws.
- `mnemosyne-backend` owns operating-system and device-facing mapping
  operations behind the memory-backend seam.
- `mnemosyne-arena` owns segments, global segment pools, scratch pools, and
  arena-local reclamation.
- `mnemosyne-local` owns the thread-local and per-CPU allocation paths.
- `mnemosyne-decay` owns cross-thread reclamation and physical-memory return.
- `mnemosyne-heap` owns explicit branded heaps and tiered allocation routes.
- `mnemosyne-prof` owns allocation sampling, leak tracking, and dump files.

The top-level `mnemosyne` crate provides the process-facing `GlobalAlloc`
adapter and re-exports the public policy, telemetry, scratch, and branded
ownership surfaces. A consumer opts into that adapter; Mnemosyne does not
silently become the global allocator for every Atlas binary. Explicit heaps
remain available when a subsystem needs a lifetime or placement boundary that
the process allocator cannot express.

Small allocation routing is defined by the implementation, not by a generic
label such as “slab”. Requests representable by a size class up to
`MAX_SMALL_ALLOC_SIZE` (currently 16 KiB), with supported alignment, use the
local path. Larger requests or requests whose alignment cannot fit that path
use segment or direct/huge allocation routes. See [size classes](size_classes.md)
and [segment lifecycle](segment_lifecycle.md) for the routing and retention
contracts.

Mnemosyne also owns the lifecycle knobs that affect retained host memory:
`decay` performs reclamation and purge work, `reset` drops standard segment
backing while retaining mappings, and `purge` releases cached mappings. The
huge-cache behavior is deliberately distinct. The [decay, purge, and reset
chapter](decay_purge_reset.md) is the contract for these operations.

## Where Mnemosyne sits

The arrows below show downstream consumption. They are provider boundaries,
not a claim that every repository depends directly on every lower layer.

```text
Themis (placement vocabulary)       Melinoe (branded capability evidence)
             \                       /
              \                     /
               +-------------------+
                         |
                         v
             Mnemosyne (allocation / arenas / staging)
                    |              |
                    v              v
        Moirai (execution)   Hermes (CPU lanes / ISA dispatch)
                                      |
                                      v
                  Leto (host arrays / views / linear algebra)
                                      |
                                      v
            Hephaestus (accelerator devices / buffers / transfers / kernels)

 Apollo (transforms / plans) --------------------> Coeus (tensors / autodiff)
       |                                             ^           ^
       +--------------------> Leto -----------------+           |
                                      Hephaestus ---------------+

 Consus (scientific persistence / formats) is a sibling domain provider.
```

The stack map in the Atlas meta-repository is the cross-repository topology
source of truth. Package manifests remain authoritative for an individual
direct dependency edge. This chapter describes the ownership boundaries those
edges implement; it does not replace either source.

## Consumer routes

| Consumer | Mnemosyne boundary | Consumer-owned responsibility |
|---|---|---|
| `moirai` | Allocator and arena storage for execution infrastructure | Scheduling, parallel and asynchronous execution, and transport |
| `hermes` | Allocator support for CPU execution infrastructure | SIMD, SWAR, ISA detection, and CPU kernels |
| `leto` | `mnemosyne-memory`-backed storage when its Mnemosyne integration is enabled | Host arrays, layouts, views, and linear algebra |
| `apollo` | Scratch pools for transform workspaces and Mnemosyne-backed/Leto output storage | Transform mathematics and plans |
| `hephaestus` | Host-side staging reaches Mnemosyne through Leto; device resources stay in Hephaestus | Accelerator devices, buffers, transfers, and kernels |
| `coeus` | Receives Mnemosyne-backed storage through Leto and Hephaestus | Tensors, autodiff, neural-network, and optimizer semantics |
| `consus` | May use the process allocator for bounded I/O buffers | Scientific persistence, serialization, compression, and transport formats |

The route for array data is therefore:

```text
algorithm/domain result
          |
          v
Leto storage contract
          |
          v
Mnemosyne allocation / arena / staging substrate
```

An algorithm should not create a second allocator-local storage family to
avoid this route. Leto owns the storage abstraction and can select borrowed,
owned, or Mnemosyne-backed storage according to the operation's contract.
Apollo owns transform outputs and plans; it consumes that storage contract
instead of making allocator policy part of transform mathematics.

Hephaestus is the exception only in the direction of device ownership: it
owns the lifetime and synchronization of accelerator resources. Mnemosyne
does not become a second `wgpu` or CUDA resource manager. Host staging is
allocated through Leto/Mnemosyne, while device buffers, command submission,
and device transfers remain in Hephaestus.

## Themis placement integration

Themis owns the placement vocabulary and locality law. Mnemosyne owns the
allocation operation that applies that vocabulary:

- the arena segment pool reads the current NUMA node and prefers its local
  bucket, refreshing the observation after bounded misses;
- `mnemosyne-heap` accepts Themis's `PlacementHint`, and
  `PlacementHint::Numa(node)` resolves to host DRAM plus an explicit NUMA
  binding attempt;
- a tier hint selects the requested memory tier, while tiers without a
  backing provider return no block rather than silently changing the request;
- cached segments retain their prior binding because reuse does not perform a
  new operating-system allocation.

The current-node path is automatic for arena segment selection. An explicit
`PlacementHint` is not inferred from a Moirai task: the caller passes it at the
heap boundary. NUMA binding is platform-dependent and returns a typed result
where the explicit heap API exposes one. The complete placement and failure
contract is in [NUMA placement](numa_placement.md).

## Melinoe branded ownership

Melinoe owns the capability evidence used by branded heaps. `mnemosyne-heap`
uses Melinoe's `ThreadLocalToken`, `SyncRegionToken`, and scoped brand APIs to
tie a heap and its allocations to one invariant lifetime. The scope prevents a
branded block from being used with another brand or escaping the scope that
owns its reclamation. The brand is a proof carried by the type; allocator
internals still contain the narrowly isolated unsafe code required to obtain
and release raw storage.

This division keeps the dependency direction unidirectional: Melinoe proves
which access is authorized, while Mnemosyne performs allocation and
reclamation. Neither crate absorbs the other's domain role.

## Boundary rules

When integrating a new Atlas consumer, keep these responsibilities separate:

1. Use Mnemosyne for allocation, arena lifetime, scratch reuse, staging, and
   memory telemetry.
2. Use Melinoe for branded capability and scope evidence.
3. Use Themis for placement and locality vocabulary.
4. Use Moirai for execution regime and transport.
5. Use Hermes for CPU lane and ISA specialization.
6. Use Leto for host storage and zero-copy views.
7. Use Hephaestus for accelerator resource ownership.
8. Use Apollo, Coeus, or Consus for their domain contracts.

This keeps one storage route, preserves static dispatch at each seam, and
prevents infrastructure details from leaking into domain algorithms.
