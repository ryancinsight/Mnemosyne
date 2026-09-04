# Segment Lifecycle

A segment is the allocator's OS-backed unit. Mnemosyne keeps the segment header
and page metadata in the first page, sub-allocates the remaining pages, and
returns an unused segment to a NUMA-aware cache before releasing it to the OS.

## Layout

The layout constants are defined once in `mnemosyne-core`:

| Constant | Value | Role |
|---|---:|---|
| `SEGMENT_SIZE` | 2 MiB | Usable, segment-aligned extent |
| `SEGMENT_ALIGN` | 2 MiB | Address-rounding alignment |
| `PAGE_SIZE` | 64 KiB | Segment page stride |
| `PAGES_PER_SEGMENT` | 32 | `SEGMENT_SIZE / PAGE_SIZE` |
| `SEGMENT_MAPPING_SIZE` | 4 MiB | Raw mapping reserved for alignment slack |

Page 0 carries the `Segment` header. Pages 1 through 31 carry size-class
pages. A fresh mapping reserves twice the usable extent so a 2 MiB-aligned
segment can be found inside it. The unused head and tail slack are decommitted
when the backend supports that operation; the segment is still released as one
4 MiB backend mapping.

## Lifecycle

```text
fresh OS mapping (4 MiB)
        |
        v
aligned and initialized segment
        |
        v
owned by a thread; pages serve small allocations
        |
        v  (all pages empty)
NUMA-aware global segment cache
        |
        +--> reused by a local-node allocation
        |
        +--> purged or over the runtime retention cap
                         |
                         v
                    backend release
```

Fresh allocation performs the cold-path work: alignment, header initialization,
NUMA binding, optional guard installation, and slack decommit. A cache hit
reinitializes the segment header and skips the OS mapping and NUMA-binding
syscalls. The local NUMA bucket is attempted first; a miss refreshes the
thread's NUMA observation at a bounded cadence and then steals from other
buckets in a fixed wrap order.

## Two cache families

`GlobalSegmentPool` retains empty standard segments partitioned into fixed NUMA
buckets. It is fed when a thread releases an empty segment and is consumed
before a fresh `SEGMENT_MAPPING_SIZE` mapping is requested. The runtime option
`max_retained_segments` is the enforced soft cap; `set_options` clamps it to
the compile-time limit `MAX_RETAINED_SEGMENTS_LIMIT` (1024). The count and byte
telemetry are advisory under concurrent push/pop operations, so a small
contention overshoot is permitted to keep the cache lock-free on the hot path.

`GlobalHugePool` caches direct allocations that cannot use a small size class:
requests above `MAX_SMALL_ALLOC_SIZE` (16 KiB), or requests whose alignment
cannot be satisfied by a size-class stride. Its buckets are logarithmic and
NUMA-partitioned. Cached blocks are limited to a 16 MiB mapping-size class and
to a per-bucket byte budget; requests that would be over-provisioned beyond the
fit cap miss the cache and use a fresh mapping. This keeps reuse from pinning a
large resident range for a small request.

## Release policy

An empty standard segment first enters `GlobalSegmentPool` if the runtime cap
has room. If admission fails, Mnemosyne releases the mapping to the backend.
An empty direct allocation follows the corresponding `GlobalHugePool` byte and
count budgets. The cache therefore trades a bounded amount of retained address
space for lower mapping latency and preserves locality by trying the originating
NUMA bucket first.

`purge` detaches retained standard segments and huge blocks, releases them to
the OS, and records the completed sweep. `purge_lazy(warm_threshold)` performs
the same sweep while retaining up to the requested number of standard segments;
the huge cache is still purged. `reset` only drops the physical backing of
retained standard segments, preserving their virtual mappings for warm reuse;
it does not reset the separate huge-allocation cache.

`decay` drives one synchronous sweep across the production backends. The
background decay worker is enabled only when `purge_cadence_ms` is nonzero; it
reclaims orphaned thread-owned segments and purges retained caches. A zero
cadence starts no worker and leaves reclamation to allocation-path admission,
explicit `purge`, `reset`, or a caller-driven `decay`.

## Measuring the trade-off

`mnemosyne::memory_stats()` exposes the counters needed to distinguish retained
address space from returned physical backing:

```rust,ignore
let before = mnemosyne::memory_stats();
// Run the allocation phase here.
let after_alloc = mnemosyne::memory_stats();
mnemosyne::reset();
let after_reset = mnemosyne::memory_stats();
mnemosyne::purge();
let after_purge = mnemosyne::memory_stats();
```

`retained_free_bytes` and `retained_huge_bytes` describe cached address space.
`page_reset_bytes` describes physical backing dropped while standard mappings
remain warm. `purged_bytes` and `unmap_calls` describe mappings returned to the
OS. Compare these fields with `fresh_segments`, `map_calls`, and the current
thread counters when choosing between warm retention and idle reclamation. The
allocator-statistics example exercises the same observable counters with a
small deterministic workload.
