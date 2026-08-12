# NUMA-Aware Placement

Mnemosyne integrates with Themis to allocate memory on the NUMA node
closest to the thread that will use it.

## Placement at Allocation Time

Mnemosyne queries the calling thread's NUMA home using Themis:

```rust,ignore
use themis::current_numa_node;

let node = current_numa_node();  // from Themis
```

The result guides segment selection: Mnemosyne prefers segments whose
physical pages were mapped from the caller's NUMA domain. If no suitable
segment is available, it falls back gracefully to a remote segment rather
than failing.

## `PlacementHint` Integration

Callers can hint at a specific NUMA node through Moirai's task placement:

```rust,ignore
runtime.spawn_fn_with(
    TaskBuilder::new().placement(PlacementHint::Numa(node_id)),
    || { /* first-touch allocation here */ },
);
```

Moirai pins the task to the target worker; the worker's first-touch
allocation then lands on its NUMA domain's physical memory.

## NUMA Bucket Table

`NumaBucketIndex<N>` (from Themis) maps NUMA node IDs into a fixed-size
bucket table used by Mnemosyne's internal per-node segment pools. The
const-generic `BUCKETS` parameter is set at compile time; zero-size
tables are rejected at compile time by the `ASSERT_NONZERO` const.

## First-Touch Policy

Mnemosyne's `MemoryBackend` trait exposes `allocate` without explicit
NUMA binding. The OS first-touch policy ensures that pages accessed by
the Moirai worker on the target NUMA node are homed there, provided the
worker performs the initialization write rather than a different thread.
