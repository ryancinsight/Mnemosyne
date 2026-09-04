# Example: Scratch Pool

**Crate**: `mnemosyne`
**Source**: `crates/mnemosyne/examples/book_scratch_pool.rs`

`ScratchPool<T>` is a reusable, aligned buffer pool for temporary computation.
Construction is zero-allocation; the first request grows a pooled slot, and
later requests reuse it while they fit the provisioned capacity. A larger
request grows the slot, and nesting beyond `MAX_POOL_SLOTS` (4) uses a
temporary owned buffer. The example also demonstrates bounded provisioning
and explicit idle reclamation.

## Source

```rust
{{#include ../../../crates/mnemosyne/examples/book_scratch_pool.rs}}
```

## Output

```text
sum of 0..1024 via scratch: 523776 (expected 523776)
windowed dot-product: 7.870039
f32 scratch sum: 64
bounded release: capacity=2048 -> retained=1025
MAX_POOL_SLOTS = 4 (max concurrent nested borrows)
all scratch-pool assertions passed
```

## What to notice

- `ScratchPool::new()` is `const fn` and allocates nothing. The first
  `with_scratch` call grows its selected `AlignedVec`; subsequent requests
  that fit the slot reuse its storage. A request larger than the current
  capacity is a real growth allocation.

- Nested borrows work because `ScratchPool` maintains a `borrow_depth: Cell<u8>`
  counter.  Each `with_scratch` increments the depth and picks a fresh slot;
  the slot is returned to the pool when the closure returns.  Up to
  `MAX_POOL_SLOTS = 4` nesting levels are supported without growing the pool.

- `with_scratch_bounded` records the request at each depth.  The example's
  1025-element request grows geometrically to 2048, then `release` returns the
  1023-element headroom while retaining exactly the recorded working set. A
  following `reset` plus `release` returns the idle slot completely.

- In production code the pool lives in `thread_local!` storage so it persists
  across calls.  The example uses a local variable to avoid the
  `clippy::missing_const_for_thread_local` false positive on clippy 1.97.0
  (ATLAS-MNEMOSYNE-CI-1).

- The `ScratchElement` bound on `T` requires `Copy`, `Send`, `Sync`, and a valid
  all-zero representation. Built-in scalar/complex implementations avoid an
  optional dependency; the `bytemuck` feature extends the same contract to
  user-defined `Zeroable` POD types.
