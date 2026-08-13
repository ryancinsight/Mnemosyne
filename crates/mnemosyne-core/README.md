# mnemosyne-memory-core

Core types, constants, size classes, and synchronization primitives for the
[Mnemosyne](https://github.com/ryancinsight/mnemosyne) allocator. The library
name is `mnemosyne_core`; the crates.io package is `mnemosyne-memory-core`.

This is the foundation crate: every other Mnemosyne crate depends on it, and it
depends on nothing in the workspace. It is `#![no_std]`.

```toml
[dependencies]
mnemosyne-memory-core = "0.2"
```

## Contents

- `policy` — the `AllocPolicy` sealed trait and its zero-sized implementations
  (`StandardPolicy`, `SecurePolicy`, `HardenedPolicy`). Policy selection is a
  type parameter, so the unused branch is dead code at compile time.
- `types` — `Page`, the free-list types, and `AtomicFreeList`, the page-local
  lock-free cross-thread free queue. `Page` is pinned to exactly one 64-byte
  cache line by a layout test.
- `size_class` — size-class derivation shared by every allocation path.
- `validation` — `is_valid_alloc_request` and `is_valid_layout_alloc_request`,
  the two `const fn` predicates every allocation entry point routes through, so
  `MAX_ALLOC_SIZE` and the alignment rules have one definition.
- `constants` — segment and page geometry (2 MiB segments, 64 KiB pages).
- `sync`, `loom_shim` — synchronization primitives, swappable for loom's
  model-checked equivalents under `cfg(loom)`.
- `abort` — the single authoritative corruption sink.

This crate defines the `MemoryBackend` contract but implements no backend; see
`mnemosyne-backend`.

Licensed under MIT OR Apache-2.0.
