# Position in the Stack

## What Mnemosyne Owns

Mnemosyne is the Atlas memory allocator layer. It owns:

- **All heap memory** for every Atlas crate — via `#[global_allocator]`
- **Thread-local slab allocators** for small objects (< 8 KB)
- **Global segment pools** for large and huge allocations
- **Background decay engine** for proactive OS memory reclamation
- **NUMA-aware placement** guided by Themis placement vocabulary
- **Branded arenas** (`mnemosyne-heap`) for scope-bounded allocations
- **Heap profiler and leak detector** for diagnostics
- **Policy-generic backend** servicing CPU RAM, CUDA, HBM, and host-pinned

Mnemosyne does **not** own scheduling (Moirai), placement vocabulary (Themis),
or storage format I/O (Consus).

## Where Mnemosyne Sits

`	ext
themis (placement vocabulary)
  |
  v
mnemosyne (heap allocator)
  |
  v (GlobalAlloc impl consumed by every crate)
coeus   apollo   moirai   consus   helios   ritk   kwavers   CFDrs
`

## Consumers

| Consumer | How Mnemosyne is used |
|----------|----------------------|
| `coeus-tensor` | Tensor storage allocation via `MnemosyneAllocator` |
| `apollo-fft` | `ScratchPool<f32/f64>` for FFT workspace buffers |
| `moirai-parallel` | Thread-local storage for worker metadata |
| `consus-io` | I/O buffer allocation |
| `hephaestus-cuda` | CUDA device memory via CUDA-specific backend |
| `leto` | Array storage backing |

## Themis Integration

Mnemosyne reads the calling thread's `NumaNodeId` from Themis to select
NUMA-local segments. `PlacementHint::Numa(id)` in a Moirai task context
causes first-touch allocation to land on the target NUMA node.

## Branded Arenas (`mnemosyne-heap`)

`mnemosyne-heap` provides GhostCell-style branded arenas: allocations
carry an invariant lifetime brand that prevents objects from escaping the
`scope` block. This makes arena deallocation provably safe without
unsafe code at the call site.
