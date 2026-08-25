# NUMA-Aware Placement

Mnemosyne integrates with Themis to keep memory close to the thread that will
use it. Themis owns the placement *vocabulary* — `NumaNodeId`, topology
detection, `PlacementHint` — and Mnemosyne owns the *execution*: the segment
pool that keeps per-node caches, and the kernel memory-policy calls that make
a hint real.

## Querying the Caller's Node

`mnemosyne-arena::numa` wraps Themis's thread-local node query:

```rust,ignore
use mnemosyne_arena::current_numa_node;            // re-exported at the crate root
use mnemosyne_arena::numa::refresh_numa_node;      // module path only

let node: u32 = current_numa_node();      // cached, cheap
let node: u32 = refresh_numa_node();      // forces an OS query
```

Every segment records the node it was first mapped from:
`Segment::initialize` stores the `current_numa_node()` value into the
`Segment::numa_node` field at `allocate_segment` time.

## Per-Node Segment Pools

`GlobalSegmentPool` is not one list — it is an array of `NUMA_BUCKETS` (16)
sub-pools, one per NUMA bucket. `numa_bucket::bucket_index` folds an arbitrary
node id into that fixed range using Themis's
`NumaNodeId::bucket_index::<NUMA_BUCKETS>()`, so a machine with more nodes
than buckets wraps rather than overflowing the table.

`GlobalSegmentPool::pop` is local-first with a bounded remote fallback
(`crates/mnemosyne-arena/src/segment/pool/segment_pool.rs`):

1. Pop from the caller's own bucket.
2. On a miss, re-query the OS node — but only once every 32 misses, so thread
   migration is picked up without paying a syscall per miss — and retry the
   caller's bucket if it changed.
3. Otherwise `steal_from` walks the other 15 buckets in wrap order and takes
   the first available segment.

A remote segment is therefore a graceful degradation, never an allocation
failure.

## Explicit Binding and First Touch

`mnemosyne-heap::numa` owns the kernel calls, and its platform contract is
explicit:

| Primitive | Linux | Windows | Other |
|-----------|-------|---------|-------|
| `bind_to_node` | `mbind(MPOL_BIND)` | documented no-op (no `mbind` equivalent for existing allocations) | no-op |
| `allocate_interleaved` | `mbind(MPOL_INTERLEAVE)` after a standard allocation | `VirtualAllocExNuma`, chunked per node, when the topology reports more than one node | plain allocation |
| `first_touch` | walks the range at a 4 KiB stride | same | same |

Binding is best-effort *by contract*: a failed policy call is a locality hint
that could not be honored, never an allocation failure. The error type exists
so an explicit caller can distinguish and log the reason.

`TieredHeap::alloc` routes `themis::PlacementHint::Numa(node)` through
`bind_to_node` internally, so tiered-heap callers do not invoke the primitives
themselves.

## First-Touch Policy

The OS decides physical placement on first write, not on mapping. So a
segment mapped by one thread and first written by another is homed on the
*writer's* node. `first_touch` exists to make that write happen on the
intended thread, at a 4 KiB stride that touches every OS page regardless of
the host page size.
