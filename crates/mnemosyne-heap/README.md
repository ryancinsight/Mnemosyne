# mnemosyne-heap

Explicit heap handles, tiered placement, and lifetime-branded allocation for the
[Mnemosyne](https://github.com/ryancinsight/mnemosyne) allocator.

```toml
[dependencies]
mnemosyne-heap = "0.4"
```

Use the global allocator for process-wide allocation; reach for this crate when
an allocation stream needs to be isolated, steered to a specific memory tier, or
tied to a scope by the type system.

## Two concerns

`Heap` is the owning allocator handle. `TieredHeap` extends it with placement
across memory tiers (`MemoryTier`, `PlacementHint`) so a caller can steer an
allocation toward the tier matching its access pattern instead of treating all
memory as uniform.

`MnemosyneHeap` and `BrandedHeap` share one internal `RawHeap<P, B>` for
allocation, free, and realloc mechanics — branding adds type-level evidence at
the API boundary, not a second copy of the allocator.

## Branding

`BrandedBox`, `BrandedVec`, `BrandedBlock`, and `BrandedCell` carry an invariant
lifetime tying an allocation to the `scope` that produced it. Because the brand
is invariant, returning a block to a different heap is a type error rather than
a runtime check. The brand marker (`InvariantLifetime`) and the thread-confined
capability token (`ThreadLocalToken`) come from
[melinoe](https://github.com/ryancinsight/melinoe), the single source of brand
machinery for this ecosystem.

`Heap::realloc` validates size and alignment before entering the raw allocator
and returns `Result<Option<_>, ReallocError>`; a failure owns the original block
so the caller can recover or release it without leaking. A zero `new_size` frees
the source and returns `Ok(None)`. `TieredHeap::realloc` preserves the carried
tier through the same contract.

Licensed under MIT OR Apache-2.0.
