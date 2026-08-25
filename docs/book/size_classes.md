# Size Classes

Mnemosyne splits every allocation request into one of two regimes at a single
threshold, then routes small requests through a fixed table of size classes so
that per-allocation overhead amortizes across many objects of similar size.

## The Two Regimes

The threshold is `MAX_SMALL_ALLOC_SIZE`, defined in
`mnemosyne-core::constants` as 8 KiB.

| Regime | Range | Mechanism |
|--------|-------|-----------|
| Small | 1 B – 8 KiB (`MAX_SMALL_ALLOC_SIZE`) | One of `NUM_SIZE_CLASSES` (44) size classes, served from a thread-local page free list |
| Large / huge | > 8 KiB | `allocate_large_or_huge` maps directly from the backend; idle huge mappings are retained per NUMA bucket in `GlobalHugePool` under a byte budget |

`size_to_class` (`mnemosyne-core::size_class`) performs the mapping. It is a
`const fn` over a compile-time-generated
`SIZE_TO_CLASS: [u8; MAX_SMALL_ALLOC_SIZE + 1]` lookup table, so classification
is a single indexed load rather than leading-zero arithmetic. A size above the
threshold returns `None`, which is the signal that routes the request to the
large/huge path.

## Geometry Constants

These live in `mnemosyne-core::constants` and are pinned by `const` assertions
in the same file:

- `MIN_BLOCK_SIZE` = 16 B — the smallest class, and a divisor of `PAGE_SIZE`.
- `PAGE_SIZE` = 64 KiB — the allocator-domain page (not the OS page). A page
  serves exactly one size class at a time.
- `SEGMENT_SIZE` = `SEGMENT_ALIGN` = 2 MiB — the unit of OS mapping and of
  pool retention. `SEGMENT_ALIGN == SEGMENT_SIZE` is what lets a free recover
  its owning segment header by address rounding, with no side table.
- `PAGES_PER_SEGMENT` = 32 — `SEGMENT_SIZE / PAGE_SIZE`.
- `MAX_RETAINED_SEGMENTS_LIMIT` = 1024 — the compile-time ceiling on retained
  segments. The effective bound is the runtime
  `MnemosyneOptions::max_retained_segments`, which defaults to that ceiling
  and is clamped to it on every `set_options` call.

## The Per-Page Free Lists

Small allocation does not use a central cache. Each `Page`
(`mnemosyne-core::types::page`) carries two free lists, following mimalloc's
free-list sharding and snmalloc's message passing:

1. `free: Option<NonNull<Block>>` — the list the owning thread pops from and
   pushes its own frees onto. No atomics.
2. `thread_free: AtomicFreeList` — the queue into which *other* threads push
   their frees. Cross-thread frees never contend on a page lock or a central
   pool; they publish into the owning page's own queue.

The fast path touches only `free`. When it empties, the allocator
batch-reclaims `thread_free` (the `Page::reclaim_thread_free_*` family in
`mnemosyne-core::types::page::reclaim`) and only then takes a cold path to
another page. Because `free`, `thread_free`, `alloc_count`, and `block_size`
all live in one 64-byte `Page` — a size pinned by
`page_struct_size_stays_within_one_cache_line` — the hot path reads and writes
a single cache line.

## What This Chapter Does Not Cover

`KernelResourceBudget` and `OccupancyLimits` (`mnemosyne-core::kernel_budget`)
are `const` occupancy limiters for GPU kernel launch shapes. They are not part
of the CPU size-class path. The allocator's own per-class statistics are
covered in the profiling chapter.
