# Segment Lifecycle

A **segment** is the unit of OS-mapped memory that Mnemosyne allocates, caches,
and eventually releases.

## Lifecycle Stages

`	ext
OS map (mmap/VirtualAlloc)
  |
  v
Active segment — sub-allocated to objects
  |
  v (all objects freed)
Free segment — returned to GlobalSegmentPool
  |
  v (pool exceeds MAX_RETAINED_SEGMENTS)
Decay / purge — segment released back to OS
`

## Segment Pools

| Pool | Description |
|------|-------------|
| `GlobalSegmentPool` | Global free cache for normal-size segments |
| `GlobalHugePool` | Global free cache for huge (> 2 MB) segments |

Pool contents are consumed by new allocation requests before the OS is asked
for fresh memory, amortizing `mmap`/`VirtualAlloc` syscall cost.

## Key Constants

- `SEGMENT_MAPPING_SIZE` — size of one OS-mapped segment
- `MAX_RETAINED_SEGMENTS` — cap on pool depth before decay

## Large and Huge Allocations

Allocations above 8 KB use `allocate_large_or_huge` and bypass the
thread-local slab. On deallocation, `deallocate_large_or_huge` re-caches
the segment if the pool has headroom, or returns it to the OS immediately.
