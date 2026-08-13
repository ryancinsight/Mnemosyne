# mnemosyne-memory

The public shell of [Mnemosyne](https://github.com/ryancinsight/mnemosyne), a
concurrent user-space memory allocator written entirely in Rust. The library
name is `mnemosyne`; the crates.io package is `mnemosyne-memory`.

```toml
[dependencies]
mnemosyne-memory = "0.7"
```

```rust
use mnemosyne::Mnemosyne;

#[global_allocator]
static ALLOCATOR: Mnemosyne = Mnemosyne;
```

## What this crate provides

- `Mnemosyne` — the global allocator, routed to `StandardPolicy`.
- `MnemosyneAllocator<P: AllocPolicy>` — the same allocator with a compile-time
  policy injected. `SecurePolicy` adds zero-initialization on allocation and
  `0xDE` poisoning at the deallocation boundary; because the policy is a
  zero-sized type, the unused branch is eliminated at compile time.
- `memory_stats()` / `backend_memory_stats()` — mapping, purge, and thread-cache
  telemetry.
- `configure()` / `get_options()` — runtime tuning of segment retention and
  purge cadence.
- `usable_size(ptr)` — the allocator's actual reservation for a pointer, the
  equivalent of `mi_usable_size` / `malloc_usable_size`.
- `reset()` / `purge()` — RSS reduction, with and without surrendering address
  space.
- `scratch` — aligned scratch lanes for `f32`, `f64`, and `u8`.
- Branded heap re-exports (`branded_scope`, `BrandedBox`, `BrandedVec`,
  `BrandedCell`) behind the `branded` feature.

The design follows mimalloc's free-list sharding and segment/page geometry and
snmalloc's message-passing cross-thread frees. See the
[repository README](https://github.com/ryancinsight/mnemosyne#readme) for the
mechanism-to-source-paper table.

Licensed under MIT OR Apache-2.0.
