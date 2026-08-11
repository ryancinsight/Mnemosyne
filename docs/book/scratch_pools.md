# Scratch Pools

`ScratchPool<T>` is a reusable, aligned buffer pool for temporary
computation.  It exists to cover the pattern where a hot loop needs a
transient buffer — FFT twiddles, solver residuals, windowing — without
paying a global-allocator round trip on every call.

## Construction is free

```rust
use mnemosyne::scratch::{ScratchPool, MAX_POOL_SLOTS};

let pool: ScratchPool<f64> = ScratchPool::new();
```

`ScratchPool::new()` is a `const fn` that allocates nothing.  The backing
`AlignedVec<T>` is mapped lazily on the first `with_scratch` call and reused
on every subsequent call within the same pool instance.  In production the
pool is stored in `thread_local!` storage (it is `Send` but not `Sync`, by
design) so it persists between calls.

## Borrowing a buffer

```rust
pool.with_scratch(1024, |scratch: &mut [f64]| {
    for (i, slot) in scratch.iter_mut().enumerate() {
        *slot = i as f64;
    }
    scratch.iter().sum::<f64>()
});
```

`with_scratch(n, f)` hands the closure a mutable slice of at least `n`
elements.  The pool supports **nested borrows up to `MAX_POOL_SLOTS = 4`**:
each nested `with_scratch` increments a `borrow_depth` counter and selects a
fresh slot, and the slot returns to the pool when the closure returns.  This
covers recursive FFT twiddle computation and nested solver residual
patterns.

`ScratchBank<T>` provides the lower-level API with an explicit slot index:

```rust
pub fn with_scratch<const INDEX: usize, R>(&self, n: usize, f: impl FnOnce(&mut [T]) -> R) -> R;
```

## The element bound

`T: ScratchElement` requires `Copy + Default + bytemuck::Zeroable`, so the
pool can zero-initialize and copy elements without `unsafe`.  `ScratchPool`
and `ScratchBank` are generic over the element type, so an `f32` pool and an
`f64` pool are independent.

## `AlignedVec`

The backing store, `AlignedVec<T>`, is a small aligned vector with a
dangling default state:

```rust
let v = AlignedVec::<u8>::dangling();   // const fn, no allocation
let v = AlignedVec::<u8>::with_capacity(64);
v.ensure_len(128);
let slice: &[u8] = v.as_slice();
let vec: Vec<u8> = v.into_vec();        // hand the buffer off when done
```

`DEFAULT_SCRATCH_ALIGN` is the default alignment used when the pool maps its
backing buffer.

See the [Scratch Pool example](examples/scratch_pool.md) for a complete,
runnable program, including the nested-borrow windowed dot-product.
