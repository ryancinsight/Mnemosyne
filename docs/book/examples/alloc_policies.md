# Example: Allocator Statistics

**Crate**: `mnemosyne`
**Source**: `crates/mnemosyne/examples/book_alloc_policies.rs`

Register `Mnemosyne` as the global allocator, allocate a set of `Vec<u64>`
across several size classes, observe the live-allocation counter and mapped-
byte accounting, then call `purge` to return free segments to the OS.

## Source

```rust
{{#include ../../../crates/mnemosyne/examples/book_alloc_policies.rs}}
```

## Output

```text
baseline: live=0
after alloc: live=6, mapped=6389760 B
after free:  live=1, mapped=6389760 B, retained_free_segments=0
after purge: retained_free_segments=0, purge_calls=1
all memory-stats assertions passed
```

## What to notice

- `#[global_allocator] static ALLOC: Mnemosyne = Mnemosyne;` is a zero-cost
  const static — `Mnemosyne` is a unit struct with no fields.  The
  `GlobalAlloc` dispatch goes through the per-thread cache without touching
  the global arena on the common path.

- `live=6` after allocating five `Vec<u64>` values (five heap allocations plus
  one internal bookkeeping allocation from `Vec::with_capacity` at warm-up).

- `mapped=6389760 B` stays constant after `drop(vecs)` because Mnemosyne
  caches free segments in a thread-local pool rather than immediately releasing
  them to the OS — the mapped range is still reserved.

- `purge()` triggers `madvise(MADV_FREE)`/`VirtualAlloc(MEM_RESET)` per free
  segment and increments `purge_calls`.  The retained-segment counter drops to
  zero when the pool was already empty (the `Vec` elements were small enough to
  fit inside a single mapped segment that stayed in use).
