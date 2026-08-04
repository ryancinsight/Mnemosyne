# Example: Scratch Pool

**Crate**: `mnemosyne`
**Source**: `crates/mnemosyne/examples/book_scratch_pool.rs`

`ScratchPool<T>` is a reusable, aligned buffer pool for temporary computation.
Construction is zero-allocation; the backing buffer is mapped only on the
first `with_scratch` call and reused on subsequent calls.  Up to
`MAX_POOL_SLOTS` (4) nested borrows are supported, covering recursive FFT
twiddle computation, nested solver residuals, and similar patterns.

## Source

```rust
{{#include ../../../crates/mnemosyne/examples/book_scratch_pool.rs}}
```

## Output

```text
sum of 0..1024 via scratch: 523776 (expected 523776)
windowed dot-product: 7.870039
f32 scratch sum: 64
MAX_POOL_SLOTS = 4 (max concurrent nested borrows)
all scratch-pool assertions passed
```

## What to notice

- `ScratchPool::new()` is `const fn` and allocates nothing.  The backing
  `AlignedVec` is lazily mapped on the first `with_scratch` call, then
  reused on every subsequent call within the same pool instance.

- Nested borrows work because `ScratchPool` maintains a `borrow_depth: Cell<u8>`
  counter.  Each `with_scratch` increments the depth and picks a fresh slot;
  the slot is returned to the pool when the closure returns.  Up to
  `MAX_POOL_SLOTS = 4` nesting levels are supported without growing the pool.

- In production code the pool lives in `thread_local!` storage so it persists
  across calls.  The example uses a local variable to avoid the
  `clippy::missing_const_for_thread_local` false positive on clippy 1.97.0
  (ATLAS-MNEMOSYNE-CI-1).

- The `ScratchElement` bound on `T` requires `Copy + Default + bytemuck::Zeroable`
  so the pool can zero-initialize and copy elements without unsafe code.
