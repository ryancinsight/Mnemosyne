# Decay, Purge, and Reset

Mnemosyne separates cache reuse, physical-memory reclamation, and virtual
mapping release. Select the operation from the lifecycle state being changed.

## `decay()`

`decay` runs one synchronous sweep across the production memory backends. It
drains orphaned thread-owned segments, reclaims cross-thread frees in them,
releases orphan segments that become empty, and purges the retained standard
and huge caches.

```rust,ignore
mnemosyne::decay();
```

This is the deterministic caller-driven entry point. It is useful for a
bounded maintenance phase or a test that must observe reclamation without a
background worker.

## `reset()`

`reset` asks the backend to drop the physical backing of retained standard
segments without removing their virtual mappings. Subsequent small allocations
can reuse the warm segment address range, while the operating system demand
faults the pages again as needed. The separate huge-allocation cache is not
reset.

```rust,ignore
mnemosyne::reset();
```

Use this when resident-set size matters but the next workload is expected to
reuse the same small-allocation topology.

## `purge()`

`purge` detaches and releases all retained standard segments and cached huge
blocks. It returns their mappings to the OS, so the next allocation must pay
the mapping cost again if no other cache or allocator can serve it.

```rust,ignore
mnemosyne::purge();
```

Use it after a large allocation spike or before an idle phase when both cached
address space and physical backing must be relinquished. The lighter
`purge_lazy(warm_threshold)` variant retains up to the requested number of
standard segments but still purges the huge cache.

## Runtime cadence

The default `purge_cadence_ms` is zero, so no background thread is started.
Enabling a positive cadence starts `mnemosyne-decay`, which runs the same
backend sweep periodically. The worker adapts its interval: it backs off after
a sweep that returns no bytes and speeds up when a sweep returns at least one
mebibyte. Configuration changes use an acquire/release read-modify-write
handshake so shutdown and restart cannot lose a wake-up.

```rust,ignore
use mnemosyne::MnemosyneOptions;

mnemosyne::configure(MnemosyneOptions {
    max_retained_segments: 1024,
    purge_cadence_ms: 5000,
    enable_hugepage_hint: true,
});
```

`max_retained_segments` is clamped to the compile-time limit. `enable_hugepage_hint`
is a Linux mapping hint; it does not change the segment lifecycle or guarantee
huge pages. Set cadence to zero to leave reclamation to cache admission,
explicit `decay`, `reset`, or `purge`.

## Observability

Use `memory_stats()` to distinguish the effects:

| Counter | Meaning |
|---|---|
| `retained_free_bytes` | Standard segment mappings still cached |
| `retained_huge_bytes` | Direct-allocation mappings still cached |
| `page_reset_bytes` | Physical backing dropped while standard mappings stay warm |
| `purged_bytes` | Standard mapping bytes returned to the OS by purge |
| `unmap_calls` | Successful backend unmaps |

Counters are process-wide snapshots and can move concurrently with allocation.
Read them before and after a controlled phase; do not infer resident memory from
`current_mapped_bytes`, which reports mapped address space even after physical
backing has been reset.
