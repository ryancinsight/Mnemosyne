# Size Classes

Mnemosyne maps every small allocation request to a *size class*: a fixed
bucket size that the allocator can serve from a dedicated page without
per-allocation metadata.  Bucketing is what lets the fast path be a handful
of pointer manipulations instead of a general-purpose first-fit search, at
the cost of rounding requests up to the class boundary.

## The class table

Small allocations are those up to

```rust
pub const MAX_SMALL_ALLOC_SIZE: usize = 8 * 1024; // 8 KiB
```

The table is precomputed into a const lookup array, `SIZE_TO_CLASS`, indexed
by request size.  The two entry points are:

```rust
pub fn size_to_class(size: usize) -> Option<usize>;
pub fn size_to_class_nonzero(size: usize) -> Option<usize>;
```

Both return `None` when the request falls outside the small-allocation range
(including zero and huge requests); `size_to_class_nonzero` additionally
rejects sizes that would round to the zero class.  The smallest block size in
the table is

```rust
pub const MIN_BLOCK_SIZE: usize = 16;
```

Classes below 16 bytes do not exist: a 1-byte request is served from the
16-byte class, and so on up to `MAX_SMALL_ALLOC_SIZE`.  The table is built at
compile time from `size_to_class_nonzero_arithmetic`, so class lookup on the
hot path is an indexed read, not a computation.

### Class geometry

- **Pages** are 64 KiB (`PAGE_SIZE`), aligned to `PAGE_ALIGN = PAGE_SIZE`.
- **Segments** are 2 MiB (`SEGMENT_SIZE`), aligned to
  `SEGMENT_ALIGN = SEGMENT_SIZE`, and hold
  `PAGES_PER_SEGMENT = SEGMENT_SIZE / PAGE_SIZE` pages.
- A page is dedicated to exactly one size class; a class's page provides
  blocks of its class size, so per-block bookkeeping is minimal.

Requests are rounded up to the next power-of-two or sub-power boundary.
Round-up means a 100-byte request costs 128 bytes of block space: allocation
succeeds quickly and realloc can often stay in class, while the cost is
internal fragmentation bounded by the class granularity.

## Why classes matter

- **Fragmentation control** — all blocks in a page are identical, so a page
  that empties can be reclaimed wholesale and a page that fills never leaks
  unusable "slack" blocks.
- **In-place realloc** — a `realloc` that stays within the same class
  (`small_realloc_fits_existing_class`) is a no-op on the block.
- **Cache friendliness** — the thread-local allocator keeps a per-class page
  cache, so small allocations rarely touch the global arena.

## Huge allocations

Requests above `MAX_SMALL_ALLOC_SIZE` do not go through the class table.
They are routed to the arena's `allocate_large_or_huge` path, which maps a
dedicated range (potentially a whole huge block from `GlobalHugePool`),
avoids the class machinery entirely, and reports its own accounting through
`ArenaMemoryStats`.
