# Decay, Purge, and Reset

Mnemosyne deliberately keeps freed memory in its thread-local and segment
pools instead of unmapping it immediately — unmapping and re-mapping on every
free would destroy the throughput the allocator exists to provide.  The
downside is that a long-lived process can retain resident memory for ranges
that were used once and never again.  Three mechanisms return that memory:
**decay** (automatic, on a cadence), **purge** (explicit, unmaps), and
**reset** (explicit, releases physical backing but keeps the reservation).

## Decay engine

`mnemosyne-decay` runs the background sweep:

```rust
pub fn init_decay_engine();
pub fn decay_step();
```

`init_decay_engine()` lazily spawns a worker thread (`mnemosyne-decay`) that
sweeps the active arenas on the configured cadence.  The cadence comes from
`MnemosyneOptions::purge_cadence_ms` (also settable via the
`MNEMOSYNE_PURGE_CADENCE_MS` environment variable); **zero disables the
engine entirely**.  `decay_step()` performs one sweep synchronously, which is
how the engine's work can be driven manually or exercised from tests.  The
top-level facade re-exports `decay()` for a single synchronous pass.

During a sweep the engine:

1. Walks cached free segments and pages looking for empties
   (`recycle_sweeps` counter).
2. Returns segments that have been idle past the cadence to the OS
   (`purged_segments`/`purged_bytes`).

## Purge

`purge()` is the explicit "give it all back" control:

```rust
use mnemosyne::purge;

purge(); // all cached free segments return to the OS
```

Internally it runs `purge_segment_pool<B>()` on the live segment pool,
applying the backend's release path (`madvise(MADV_FREE)` /
`VirtualAlloc(MEM_RESET)` semantics at the mapping layer).  Mapped ranges
that are still live are untouched.  `MemoryStats` tracks
`purge_calls`, `purged_segments`, and `purged_bytes`.

## Reset

`reset()` is the low-resident-memory control for long-lived processes:

```rust
use mnemosyne::reset;

reset();
```

Where purge *unmaps*, reset **releases physical backing while keeping the
address space reserved**.  It runs `reset_segment_pool<B>()`, which calls the
backend's `page_reset` seam — `MADV_DONTNEED` on Linux, `MADV_FREE` on
macOS/FreeBSD, `VirtualAlloc(MEM_RESET)` on Windows.  The benefit over a
full unmap is that a subsequent reuse of the same segment does not require a
new system call to re-map; the cost is that the pages are no longer resident.
`MemoryStats` tracks `page_reset_calls`/`page_reset_bytes` (confirmed backend
calls) and `reset_segments`/`reset_calls`.

## Which one do I want?

| Situation | Use |
| --- | --- |
| Batch process, peak memory irrelevant, throughput first | nothing (default) |
| Long-lived server, want idle memory returned automatically | decay engine with a cadence |
| Known quiet point; return everything now | `purge()` |
| Low-resident watermark without future re-map cost | `reset()` |

All three are observable through `memory_stats()` — the counters let you
verify that the mechanism you invoked actually moved memory.
