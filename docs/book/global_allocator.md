# Global Allocator

Mnemosyne registers as Rust's global allocator, intercepting all heap
operations from the standard library.

## Registration

```rust,ignore
use mnemosyne::Mnemosyne;

#[global_allocator]
static ALLOC: Mnemosyne = Mnemosyne;
```

`Mnemosyne` is a unit struct that implements `GlobalAlloc`. It dispatches
to `StandardPolicy` plus the platform's default `MemoryBackend`.

## `MnemosyneAllocator<P, B>`

For custom policy or backend:

```rust,ignore
use mnemosyne::{MnemosyneAllocator, SecurePolicy};

#[global_allocator]
static ALLOC: MnemosyneAllocator<SecurePolicy, _> = MnemosyneAllocator::new();
```

## Runtime Configuration

`configure(opts)` sets `MnemosyneOptions` after startup:

| Field | Default | Description |
|-------|---------|-------------|
| `max_retained_segments` | platform default | Cap on cached free segments |
| `purge_cadence_ms` | 0 (disabled) | Background decay thread interval |
| `enable_hugepage_hint` | false | Advise OS to use huge pages |

Setting a non-zero `purge_cadence_ms` spawns the background decay thread
(`mnemosyne-decay`).

## Capacity Query

```rust,ignore
let usable = mnemosyne::usable_size(ptr);
```

Returns the usable capacity of an existing allocation — may exceed the
requested size due to size-class rounding.
