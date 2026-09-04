# mnemosyne — High-Performance Memory Allocator

`mnemosyne` is the memory substrate of the Atlas stack. It provides a
concurrent user-space allocator with local caches, size-class bucketing,
NUMA-aware segment selection, scratch pools for temporary buffers, and a
policy-parametric API that lets callers trade safety for throughput at compile
time.

## Design goals

- **Local, low-contention** — requests representable by a size class up to
  16 KiB, subject to alignment, are served from local caches; the global
  segment pool is touched on a refill or other route transition.
- **Size-class bucketing** — requests are rounded up to the next supported
  class in a fixed table, reducing fragmentation and enabling in-place realloc.
- **Policy parametric** — `StandardPolicy` is the no-mitigation path;
  `SecurePolicy` enables zero-initialization, poisoning, and allocation-order
  randomization; `HardenedPolicy` adds free-list encryption. The policy is a
  zero-sized compile-time parameter, so its branches specialize statically.
- **Scratch pools** — `ScratchPool<T>` is a single-owner, `Send` but not
  `Sync` reusable buffer pool for temporary computation. Construction is
  allocation-free; warm pooled calls reuse storage, while growth and nested
  calls beyond the four pooled slots allocate. `with_scratch_bounded` and
  quiescent release bound retained capacity without putting reclamation on
  every borrow exit.

## What this book covers

1. The size-class table, its rounding bound, and why it matters for
   fragmentation and page density.
2. Allocation policies as compile-time ZST parameters, including their active
   mitigation masks and zeroing/poisoning costs.
3. The global allocator: `#[global_allocator] static A: Mnemosyne = Mnemosyne`.
4. Scratch pools: reusable temporary buffers with nested borrowing and
   quiescent reclamation.
5. Segment lifecycle: how free segments are cached and reused.
6. Decay, purge, and reset: how retained host memory returns to the system.
7. NUMA placement and the `themis::PlacementHint` integration.
8. Hardened and secure policies for safety-critical subsystems.
9. Profiling hooks and the leak detector.
10. The position of Mnemosyne in the Atlas provider stack.
