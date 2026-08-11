# Segment Lifecycle

The arena manages memory in **segments**: 2 MiB, 2 MiB-aligned mappings
(`SEGMENT_SIZE`, `SEGMENT_ALIGN`) divided into 64 KiB pages
(`PAGE_SIZE`, 32 pages per segment).  Segments are the unit of mapping,
retention, decay, and reset — the thread-local allocator hands out pages
from a segment, and a segment returns to the pool when all its pages are
free.

## Allocation

A fresh segment is mapped by the backend (`DefaultBackend`, or a CUDA
backend for device memory) through `mnemosyne-arena`'s pool machinery:

```rust
pub unsafe fn allocate_segment<B: HasSegmentPool>() -> Option<*mut Segment>;
```

Segments can carry **guard pages** installed via the backend `make_guard`
seam — `mprotect(PROT_NONE)` on Unix, `VirtualProtect(PAGE_NOACCESS)` on
Windows — so an out-of-bounds write at a segment boundary traps instead of
silently corrupting an adjacent mapping.  Guards are opt-in features of the
`mnemosyne-arena` crate, matching the `mnemosyne` facade features of the
same names:

- `segment-tail-guards` — a 4 KiB guard at `aligned_addr + SEGMENT_SIZE`,
  past the last page, catches forward overflow.
- `segment-header-guards` — a 4 KiB guard in the page-0 padding at
  `aligned_addr + PAGE_SIZE - 4096`, catches writes into the segment
  header.

Each is installed when the feature is enabled **and** the active backend
reports `B::SUPPORTS_MAKE_GUARD` (the `DefaultBackend` wrapper does).
Neither feature is on by default; see the `mnemosyne` facade's
`segment-tail-guards` / `segment-header-guards` feature flags.  The two
constants `SEGMENT_TAIL_GUARD_SIZE` and `SEGMENT_HEADER_GUARD_SIZE` are
both 4096.

## Thread ownership

A thread that exhausts its current segment requests another from the pool.
Segments owned by a thread are allocated from *without coordination*;
`current_thread_owned_segments` in `MemoryStats` counts them.  When a thread
exits, its owned segments are **orphaned and adopted** by the pool
(`orphan_segments_adopted` counter), so their memory is not leaked.

## Free and the retained pool

When a segment's last page empties, the segment does *not* immediately
return to the OS.  It is cached in the free-segment pool (up to
`MAX_RETAINED_SEGMENTS = MAX_RETAINED_SEGMENTS_LIMIT`), so a subsequent
allocation can reuse the already-mapped range.  The pool is exposed as:

```rust
pub unsafe fn deallocate_segment<B: HasSegmentPool>(segment: *mut Segment);
pub unsafe fn release_segment_mapping<B: HasSegmentPool>(segment: *mut Segment) -> SegmentRelease;
pub unsafe fn purge_segment_pool<B: HasSegmentPool>();
pub unsafe fn reset_segment_pool<B: HasSegmentPool>();
pub fn arena_memory_stats<B: HasSegmentPool>() -> ArenaMemoryStats;
```

- `release_segment_mapping` unmaps one segment and reports the release.
- `purge_segment_pool` returns *all* cached free segments to the OS.
- `reset_segment_pool` releases the physical backing of cached segments
  while keeping the address space reserved (`page_reset`).
- `arena_memory_stats` reports mapped, retained, and huge-pool accounting.

## Huge allocations

Requests above the size-class table are handled separately by
`allocate_large_or_huge`/`deallocate_large_or_huge`, which consult
`GlobalHugePool` and track retained huge blocks per NUMA node
(`retained_huge_blocks`/`retained_huge_bytes` in `MemoryStats`).

## Lifecycle at a glance

```text
allocate_segment ──▶ thread-owned segment (pages carved on demand)
       │
       ├── all pages freed ──▶ cached in free-segment pool (retained)
       │                            ├── purge()  ──▶ unmapped to OS
       │                            └── reset()  ──▶ backing released,
       │                                               address space kept
       └── thread exit ──▶ orphan adopted by pool

huge request ──▶ GlobalHugePool ──▶ retained huge block (per NUMA node)
```

The segment pool is the surface that [Decay, Purge, and
Reset](decay_purge_reset.md) operates on, and `ArenaMemoryStats` feeds the
top-level `memory_stats()` snapshot.
