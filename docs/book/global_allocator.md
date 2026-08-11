# The Global Allocator

`Mnemosyne` is a zero-sized unit struct that implements Rust's
`GlobalAlloc`, so it can be installed as the process-wide allocator with a
single static:

```rust
use mnemosyne::Mnemosyne;

#[global_allocator]
static ALLOC: Mnemosyne = Mnemosyne;
```

Because the static has no fields, installation is free — there is no
initialization routine, no global constructor, and no heap bootstrap to get
wrong.  The `GlobalAlloc` entry points (alloc, dealloc, realloc, alloc_zeroed)
dispatch to the calling thread's cache on the common path and touch the
global arena only on a cache miss.

The facade also exports `MnemosyneAllocator` for code that needs a named
handle, and everything else the top-level crate re-exports: policies, the
branded heap (`Heap`, `BrandedBox`, `BrandedVec`), scratch pools, the
thread-local selector, profiler hooks, and statistics.

## Runtime configuration

```rust
use mnemosyne::{configure, get_options, MnemosyneOptions};

let mut options = get_options();
options.max_retained_segments = 512;
options.purge_cadence_ms = 1_000;
options.enable_hugepage_hint = true;
configure(options);
```

The `MnemosyneOptions` struct has three fields:

| Field | Meaning |
| --- | --- |
| `max_retained_segments` | Cap on free segments cached for reuse (see `MAX_RETAINED_SEGMENTS_LIMIT`, default 1024). |
| `purge_cadence_ms` | Cadence of the background decay engine; `0` disables decay. |
| `enable_hugepage_hint` | Request `MADV_HUGEPAGE` for segment-sized mappings. |

`get_options()` returns a snapshot; `configure()` updates the global
settings.  These are *runtime* knobs layered on top of the compile-time
policy flags — policy is a type, options are a value.

## Statistics, purge, and reset

```rust
use mnemosyne::{memory_stats, purge, reset, decay};
```

- `memory_stats() -> MemoryStats` — a rich snapshot including
  `current_mapped_bytes`/`peak_mapped_bytes`, `retained_free_segments`,
  `purge_calls`/`purged_segments`/`purged_bytes`,
  `page_reset_calls`/`reset_segments`, `guard_install_calls`,
  `current_thread_live_allocations`, and per-thread pool telemetry
  (`page_refills`, `recycled_pages`, `fresh_pages`, `fresh_segments`,
  `orphan_segments_adopted`).
- `purge()` — return all cached free segments to the OS (`madvise(MADV_FREE)`
  / `VirtualAlloc(MEM_RESET)`), leaving mapped ranges that are still live
  untouched.
- `reset()` — release the *physical backing* of cached segments via the
  backend `page_reset` seam while keeping the address space reserved, for
  long-lived processes that want resident memory low without re-mapping cost.
- `decay()` — run a single background-decay sweep synchronously (the same
  work the decay engine performs on its cadence).

These are the observability and lifecycle knobs used by the
[Allocator Statistics example](examples/alloc_policies.md), which registers
the global allocator, allocates across size classes, and watches live and
mapped counters move.

## What you get for free

Because every `Vec`, `Box`, and `HashMap` in the process routes through the
thread-local cache, the global allocator gives the whole program the
per-thread low-contention fast path, size-class bucketing, and policy
hardening with no per-call `unsafe` on the consumer side.
