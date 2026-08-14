# ADR 0005: Sharded Active-Sample Occupancy

Status: accepted

## Context

The profiler's active-sample store already partitions its maps across 64
cache-line-aligned mutex shards. Every sampled allocation and removal also
updated one process-global `AtomicUsize`, so concurrent leak-detector callbacks
contended on a cache line unrelated to the map lock that serialized their
sample. Removal called the map-initializing accessor, which could allocate an
empty hash map when a free probed a shard that had never received a sample.

## Decision

Store one `AtomicBool` occupancy flag in each aligned shard. Insertion sets the
flag while holding that shard's mutex after a new map entry is installed.
Removal takes the shard mutex directly, returns immediately for an uninitialized
map, and clears the flag only after removing the last entry while still holding
the mutex. `on_free` routes the pointer to its shard and loads only that flag
before entering the cold removal path. The existing pointer routing remains
unchanged; this increment does not claim a mixed-hash improvement.

The flag is a fast-path presence signal, not a second ownership mechanism. Map
membership remains the authoritative sample state, and the existing allocator
pointer-lifetime synchronization contract orders a sampled allocation before
its corresponding free. A stale `true` load is harmless because removal is
value-checked under the shard mutex; a free on an inactive shard cannot find a
sample in another shard.

The report snapshot no longer sizes its output from a global counter. It grows
from an empty vector on this cold reporting path, avoiding a hot-path accounting
counter; report output values and public APIs are unchanged.

## Rejected alternatives

- Retaining the global counter preserves exact capacity hints but retains one
  shared read-modify-write line on every sampled allocation and free.
- Replacing pointer routing with a mixed hash was not selected because this
  increment's matched A/B isolated a measurable global-accounting cost without
  establishing a routing-distribution shortfall. It remains a separate
  measurement candidate.
- A process-global occupancy bit would still centralize writes and reproduce
  the same cache-line contention at a different operation name.

## Verification

The focused `mnemosyne-prof` nextest suite passes 15/15 and warning-denied
Clippy is clean. The matched four-thread small-allocation Criterion baseline
was `[10.215, 11.440, 12.975] us` with profiling disabled and
`[2.2952, 2.3488, 2.4254] ms` with leak detection enabled. After the change,
the rows were `[9.9740, 10.668, 11.417] us` (`p = 0.67`) and
`[2.2389, 2.2623, 2.2816] ms` (`-4.7386%`, `p = 0.00`), respectively. These
are empirical A/B results; the administrator-only Windows flamegraph failure
(`NotAnAdmin`) is not treated as profiling evidence.
