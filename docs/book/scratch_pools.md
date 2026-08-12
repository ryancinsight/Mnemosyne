# Scratch Pools

`ScratchPool<T>` provides reusable, aligned temporary buffers without
heap allocation on the hot path. It is designed for `thread_local!` use.

## Construction

```rust,ignore
use mnemosyne::ScratchPool;

thread_local! {
    static SCRATCH: ScratchPool<f32> = ScratchPool::new();
}
```

## Borrowing a Scratch Buffer

```rust,ignore
SCRATCH.with(|pool| {
    pool.with_scratch(1024, |buf: &mut [f32]| {
        // buf is a cache-line-aligned slice of 1024 f32 values
        do_fft(buf);
    });
});
```

`with_scratch(n, closure)` borrows an aligned `&mut [T]` of length `n`
for the closure's duration. Up to `MAX_POOL_SLOTS` (4) nested levels are
supported, enabling recursive and re-entrant usage.

## Design

- **No allocation on the hot path**: slots are pre-allocated and reused
  across invocations.
- **Cache-aligned**: every buffer starts on a cache-line boundary to
  avoid false sharing and satisfy SIMD alignment requirements.
- **Bounded nesting**: the 4-slot limit gives a fixed, compile-time-bounded
  memory cost.

## Consumers

`coeus-tensor` and `apollo-fft` use `ScratchPool` for temporary FFT
buffers to avoid repeated heap allocation during forward passes.
