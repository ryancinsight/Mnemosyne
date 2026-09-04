# Scratch Pools

`ScratchPool<T>` provides reusable, aligned temporary buffers. A pool can
reuse storage without allocation after it has been warmed for a stable
workload. It is designed for `thread_local!` use and is `Send` but
intentionally not `Sync`.

## Construction

```rust,ignore
use mnemosyne::ScratchPool;

thread_local! {
    static SCRATCH: ScratchPool<f32> = const { ScratchPool::new() };
}
```

## Borrowing a Scratch Buffer

```rust,ignore
SCRATCH.with(|pool| {
    pool.with_scratch(1024, |buf: &mut [f32]| {
        // buf is a 64-byte-aligned slice of exactly 1024 f32 values
        do_fft(buf);
    });
});
```

`with_scratch(n, closure)` borrows an aligned `&mut [T]` of length `n`
for the closure's duration. Up to `MAX_POOL_SLOTS` (4) nested levels use pooled
slots; deeper nesting uses a temporary owned buffer so the closure still gets
the requested slice but that call allocates.

For consumers that need bounded idle retention, use
`with_scratch_bounded`. It records each depth's largest request. A later
`release` can shrink geometric growth headroom above those provisions while
preserving the working set for the next warm pass:

```rust,ignore
pool.with_scratch_bounded(1024, |_| {});
pool.with_scratch_bounded(1025, |_| {}); // geometric capacity becomes 2048
assert_eq!(pool.release()[0], 1025);      // headroom is returned
pool.reset();
assert_eq!(pool.release()[0], 0);          // next idle release frees all
```

`release` skips busy slots and never invalidates a live borrow. Call it at a
consumer-selected quiescent point, not on every closure exit. `reset` clears
the recorded provisions; the following release reclaims every idle slot.

## Design

- **Warm reuse**: pooled slots reuse their `AlignedVec` storage across
  invocations after the largest workload has been provisioned; a larger
  request grows its slot, and nesting beyond four slots uses a temporary
  allocation. `prewarm` moves known growth outside a measurement window.
- **Explicit alignment**: each buffer starts at the crate's 64-byte alignment,
  satisfying the scratch element's SIMD contract. The pool is thread-local, so
  this alignment is not presented as inter-thread false-sharing protection.
- **Bounded pooled storage**: four slots are embedded in the pool. Recursion
  beyond that bound has an explicit temporary-allocation cost rather than
  silently growing the persistent pool.
- **Zeroed growth**: `AlignedVec` zero-initializes only its newly exposed range;
  warm reuse does not memset the existing working set.
- **POD element contract**: `ScratchElement` admits only copyable types with a
  valid all-zero representation. The optional `bytemuck` feature extends this
  to user-defined `Zeroable` POD types, and the optional `eunomia` feature
  covers Eunomia complex values.

## Banks and uninitialized kernels

`ScratchBank<T, N>` groups `N` pools for distinct roles such as transform input,
weights, and output. The role index is a const generic, so the bank has no
runtime role dispatch; `release` and `reset` operate on all roles.

`with_scratch_uninit` is the allocation-free kernel escape hatch. It returns a
raw slice pointer and requires the closure to initialize every element before
any safe read. Use it only when a kernel will overwrite the complete region;
ordinary `with_scratch` is the safe default.

## Consumers

Apollo's wavelet and NUFFT paths use `ScratchPool`, and Apollo FFT uses
`ScratchBank` to keep transform-role buffers thread-local and reusable. The
same substrate is available to other consumers without importing allocator
internals.
