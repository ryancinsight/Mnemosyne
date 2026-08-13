# mnemosyne-hardened

Security-hardened allocation policies for the
[Mnemosyne](https://github.com/ryancinsight/mnemosyne) allocator.

```toml
[dependencies]
mnemosyne-hardened = "0.2"
```

This crate holds no logic. `SecurePolicy` and `HardenedPolicy` are defined in
[`mnemosyne_core::policy`](https://docs.rs/mnemosyne-memory-core), alongside
`StandardPolicy`, which is the single authoritative home for `AllocPolicy`
implementations. This crate re-exports them under their historical path so
existing `use mnemosyne_hardened::{SecurePolicy, HardenedPolicy}` imports and
downstream manifests continue to resolve.

New code should depend on `mnemosyne-memory-core` (or the `mnemosyne-memory`
facade, which re-exports both policies) and use this crate only for
compatibility with an existing dependency graph.

## What the policies do

`SecurePolicy` zero-initializes on allocation and poisons the freed payload with
`0xDE` before the block is linked back into the free list, so the poison cannot
corrupt the inline next-pointer. Because the policy is a zero-sized type
selected at compile time, an application that does not use it pays nothing.

Licensed under MIT OR Apache-2.0.
