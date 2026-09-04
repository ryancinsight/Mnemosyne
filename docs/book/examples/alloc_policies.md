# Example: Allocator Statistics

**Crate**: `mnemosyne`
**Source**: `crates/mnemosyne/examples/book_alloc_policies.rs`

Register `Mnemosyne` as the global allocator, allocate a set of `Vec<u64>`
across several size classes, observe the live-allocation counter and mapped-
The example also prints the active compile-time mitigation mask for each
built-in policy; only `StandardPolicy` is installed as the process allocator.

## Source

```rust
{{#include ../../../crates/mnemosyne/examples/book_alloc_policies.rs}}
```

## Output

```text
policy standard: zst=0, active_flags=0x00, warm_segments=4
policy secure: zst=0, active_flags=0x0B, warm_segments=0
policy hardened: zst=0, active_flags=0x0F, warm_segments=0
baseline: live=1
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

- `live=6` after allocating five `Vec<u64>` values. The warmed process keeps
  one baseline runtime allocation on the measured host, so the live counter
  rises by five from `baseline: live=1`.

- `mapped=6389760 B` stays constant after `drop(vecs)` because the freed blocks
  return to thread-local free lists while the segment backing them stays in
  use; the mapped range is never released here. Note the run reports
  `retained_free_segments=0` — nothing is cached as a *free* segment in this
  example, so the constant `mapped` figure is an in-use segment, not a pooled
  one.

- The three policy lines are static metadata emitted by one generic helper.
  `zst=0` confirms that policy selection carries no instance storage, while the
  mask distinguishes the hardening work that the selected policy will execute.

- `purge()` triggers `madvise(MADV_FREE)`/`VirtualAlloc(MEM_RESET)` per free
  segment and increments `purge_calls`.  The retained-segment counter drops to
  zero when the pool was already empty (the `Vec` elements were small enough to
  fit inside a single mapped segment that stayed in use).
