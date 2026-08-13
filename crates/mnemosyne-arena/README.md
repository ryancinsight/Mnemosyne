# mnemosyne-arena

Segment arenas, page slicing, and orphan pools for the
[Mnemosyne](https://github.com/ryancinsight/mnemosyne) allocator.

```toml
[dependencies]
mnemosyne-arena = "0.4"
```

## Geometry

2 MiB segments are sliced into 64 KiB pages, following mimalloc's layout. The
parent segment of any pointer is recovered by rounding its address down to
`SEGMENT_ALIGN` — there is no side table and no per-page back-pointer.

## Orphaned segment adoption

When a thread terminates, its partially occupied segments are pushed to
`GLOBAL_ORPHAN_POOL`, a tagged-pointer intrusive stack whose head mutations are
serialized by a per-stack lock. The lock is deliberate: the mutation tag rejects
a stale CAS but cannot stop a concurrent decay sweep from releasing the observed
mapping before `pop` dereferences its successor link. This pool is therefore not
lock-free — the page-local cross-thread free queue in `mnemosyne-core` is.

Active threads scan the pool and adopt orphaned segments, repurposing empty
pages across size classes and resuming from partially filled ones, so
address space is not leaked on thread exit.

## Huge allocations

`allocate_large_or_huge` reserves exactly
`size + alignment + SEGMENT_ALIGN + PAGE_SIZE`, derived from a layout walk over
the worst-case slacks. The bound is pinned by a test asserting the exact backend
telemetry delta.

## Scratch lanes

`scratch::ScratchPool<T>` and `AlignedVec<T>` provide aligned reusable buffers
for `f32`, `f64`, and `u8`. Complex lanes are available through the `eunomia`
feature using `eunomia::Complex<f32>` / `eunomia::Complex<f64>`.

Licensed under MIT OR Apache-2.0.
