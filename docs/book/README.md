# mnemosyne — High-Performance Memory Allocator

`mnemosyne` is the memory allocator of the Atlas stack.  It provides a
concurrent user-space allocator with per-thread caches, size-class bucketing,
NUMA-aware placement, scratch pools for temporary buffers, and a policy-
parametric API that lets callers trade safety for throughput at compile time.

## Design goals

- **Per-thread, low-contention** — small allocations are served from a
  thread-local cache; the global segment pool is touched only on cache miss.
- **Size-class bucketing** — requests are rounded up to the next power-of-two
  or sub-power boundary, reducing fragmentation and enabling in-place realloc.
- **Policy parametric** — `StandardPolicy` is the fast path; `HardenedPolicy`
  adds use-after-free detection; `SecurePolicy` zeroes on both allocate and
  free for sensitive buffers.  The policy is a ZST compile-time parameter —
  switching policies has zero runtime overhead on the fast path.
- **Scratch pools** — `ScratchPool<T>` is a thread-safe, zero-allocation-at-
  construction reusable buffer pool for temporary computation.  It covers the
  FFT twiddle and solver residual patterns without touching the global
  allocator on every call.

## What this book covers

1. The size-class table and why it matters for fragmentation.
2. Allocation policies as compile-time ZST parameters.
3. The global allocator: `#[global_allocator] static A: Mnemosyne = Mnemosyne`.
4. Scratch pools: zero-allocation temporary buffers with nested borrow support.
5. Segment lifecycle: how free segments are cached, decayed, purged, and reset.
6. NUMA placement and the `themis::PlacementHint` integration.
7. Hardened and secure policies for safety-critical subsystems.
8. Profiling hooks and the leak detector.
