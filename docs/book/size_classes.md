# Size Classes

Mnemosyne uses a tiered size-class system to amortize allocation overhead
and minimize fragmentation across the full range of object sizes.

## Size Tiers

| Tier | Range | Mechanism |
|------|-------|-----------|
| Small | ≤ 256 bytes | Thread-local slab allocator; O(1) free-list pop |
| Medium | 256 B – 8 KB | Per-thread magazine with periodic background steal |
| Large | 8 KB – 2 MB | Segment sub-allocation from `GlobalSegmentPool` |
| Huge | > 2 MB | Direct OS mapping; cached in `GlobalHugePool` |

## Key Constants

- `SEGMENT_MAPPING_SIZE` — size of a single OS-mapped segment (the unit
  of large allocation and pool retention).
- `MAX_RETAINED_SEGMENTS` — cap on the number of free segments kept in
  the global pool before being returned to the OS.

## Per-Size-Class Occupancy Limits

`KernelResourceBudget` and `OccupancyLimits` (from `mnemosyne-core`) track
the live allocation count and byte footprint per size class. When a class exceeds
its occupancy limit, the background decay engine reclaims surplus segments.

## Thread-Local Magazine Protocol

Small and medium allocations use a magazine protocol:
1. A thread fills its local magazine from the global depot.
2. Allocations pop from the magazine without any synchronization.
3. When the magazine empties, it is atomically swapped back to the depot and
   a full magazine is loaded.

This eliminates lock contention on the hot allocation path.
