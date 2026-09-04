# Global Allocator

Mnemosyne provides a zero-sized adapter for Rust's `GlobalAlloc` contract.
Every allocation request enters the same validated routing path: thread-local
cache first, then the arena's page and segment machinery, with large and huge
requests routed outside the size-class table.

## Registration

```rust,ignore
use mnemosyne::Mnemosyne;

#[global_allocator]
static ALLOC: Mnemosyne = Mnemosyne;
```

`Mnemosyne` is the standard-policy CPU allocator. Its `alloc`, `dealloc`, and
`realloc` implementations are thin, monomorphized calls to the policy- and
backend-generic local allocator; the unit struct carries no runtime state.

## `MnemosyneAllocator<P, B>`

For an explicit policy, use the generic allocator. The backend defaults to
`MemoryBackendWrapper`:

```rust,ignore
use mnemosyne::{MnemosyneAllocator, SecurePolicy};

#[global_allocator]
static ALLOC: MnemosyneAllocator<SecurePolicy> = MnemosyneAllocator::new();
```

`P` controls compile-time initialization, poisoning, free-list encoding, and
page allocation order. `B` supplies the compatible segment-pool and local
allocator implementation. Policy and backend are independent variation
dimensions, so selecting either does not add a vtable or a per-allocation mode
branch.

## Reallocation and memory reuse

Mnemosyne's `realloc` keeps the existing block when the requested size remains
inside its current size class or usable mapping. This avoids an allocation,
copy, and free sequence in the common growth case. A request that crosses a
class boundary, needs a significant shrink, or changes the required alignment
uses an allocate-copy-free replacement and copies only
`min(old_size, new_size)` bytes.

The in-place path preserves the selected policy: a secure or hardened policy
zeroes newly exposed bytes, while a poisoned policy marks newly exposed and
truncated ranges according to its policy contract. The old pointer is consumed
by a successful replacement; callers must use only the returned pointer.

These cases sit *below* the `GlobalAlloc` contract rather than inside it.
That trait requires the caller to pass a non-zero `Layout` to `alloc` and a
`new_size` greater than zero to `realloc`, with `realloc`'s pointer being a
live allocation from this same allocator; violating any of those is undefined
behaviour, so a conforming caller never reaches the cases below.

The wrapper handles them anyway, as internal defensive behaviour and not as a
guarantee the trait extends: a zero-size request returns null, a null pointer
passed to `realloc` acts as an allocation when the new size is nonzero, and a
non-null pointer with a zero new size is freed and returns null. The allocator
test suite exercises these paths directly rather than through the trait.

## Runtime configuration

`configure(opts)` changes subsequent allocator operations through three global,
thread-safe atomic settings. `get_options` reads their current values as a
point-in-time snapshot; concurrent reconfiguration may update the fields
between those individual reads.

| Field | Default | Effect |
| --- | --- | --- |
| `max_retained_segments` | `MAX_RETAINED_SEGMENTS_LIMIT` (`0` under Miri) | Bounds cached free segments |
| `purge_cadence_ms` | `0` | Keeps background decay disabled |
| `enable_hugepage_hint` | `true` | Enables the Linux huge-page mapping hint |

Setting a non-zero `purge_cadence_ms` starts the background decay worker when
one is not already running. The worker is bounded by the same retained-segment
limit; setting the cadence to zero stops future automatic starts but does not
retroactively undo a worker that has already been initialized.

Use [`purge`] when all retained free segments must be released, [`reset`] when
their virtual ranges should remain reusable while physical backing is dropped,
and [`purge_lazy`] when a bounded warm cache should remain committed.

## Capacity query

```rust,ignore
let usable = unsafe { mnemosyne::usable_size(ptr) };
```

`usable_size` returns the capacity of an existing allocator-owned allocation.
For small allocations it is the selected class stride, which may exceed the
requested size. The pointer must be live and owned by the queried allocator;
foreign, null, or already-freed pointers do not satisfy the function's safety
contract.

[`purge`]: https://docs.rs/mnemosyne/latest/mnemosyne/fn.purge.html
[`reset`]: https://docs.rs/mnemosyne/latest/mnemosyne/fn.reset.html
[`purge_lazy`]: https://docs.rs/mnemosyne/latest/mnemosyne/fn.purge_lazy.html
