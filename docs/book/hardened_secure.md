# Hardened and Secure Policies

Safety-critical subsystems get their security posture from the policy
parameter, not from auditing every call site.  `mnemosyne-hardened`
re-exports the two hardened policies from `mnemosyne-core`:

```rust
pub use mnemosyne_core::policy::{HardenedPolicy, SecurePolicy};
```

Both are drop-in `AllocPolicy` implementors — see [Allocation
Policies](alloc_policies.md) for the flag matrix.  This chapter covers the
*runtime behavior* those flags produce.

## Poisoning (use-after-free and uninitialized-read detection)

With `ENABLE_POISONING = true`, every block is filled with
`POISON_ALLOC_BYTE` (0xAD) when handed out and `POISON_FREE_BYTE` (0xDE)
when freed.  Consequences:

- Reading a freed block yields a loud 0xDE pattern instead of stale data.
- Reading an uninitialized block yields 0xAD instead of whatever was there.
- Corruption and use-after-free become observable in tests and sanitizer
  runs rather than producing silent garbage.

Poisoning applies across the thread-local allocator, the arena, and the
branded heap, because they share the policy type.

## Zero-initialization

With `ZERO_INITIALIZE = true`, every allocation is zeroed on hand-out.
This is the behavior to choose for buffers that must never leak previous
contents — key material, credentials, intermediate values from a prior
tenant's computation.

## Free-list encryption

`HardenedPolicy` sets `ENABLE_FREE_LIST_ENCRYPTION = true`: free-list
pointers are stored in an obfuscated, per-segment-keyed form.  A classic
heap exploit corrupts a freed block's first word to make the allocator hand
out an arbitrary address; encrypted free lists make the stored pointer
meaningless without the segment's key, closing that primitive.

The thread-local caches key their pages by **backend and free-list
encryption mode**, so a `HardenedPolicy` free chain can never be spliced
into a `StandardPolicy` chain by a corrupted pointer — the two modes are
structurally incompatible.

## Randomized allocation

Both hardened policies set `RANDOMIZE_ALLOCATION = true`, perturbing the
order in which blocks are handed out from a page.  Deterministic layout is
the foundation of many exploitation techniques; randomization breaks the
attacker's ability to predict where the next allocation will land.

## Guard pages

Guard pages are independent of the policy flags: the arena can install a
4 KiB guard through the backend `make_guard` seam — `mprotect(PROT_NONE)`
on Unix, `VirtualProtect(PAGE_NOACCESS)` on Windows — so an out-of-bounds
write at a segment boundary traps instead of corrupting an adjacent
mapping.  They are opt-in `mnemosyne`/`mnemosyne-arena` features:

- `segment-tail-guards` — guard at `aligned_addr + SEGMENT_SIZE`, past the
  last page (catches forward overflow).
- `segment-header-guards` — guard in the page-0 padding at
  `aligned_addr + PAGE_SIZE - 4096` (catches writes into the segment
  header).

Each installs only when the feature is enabled and the active backend
supports `make_guard`.  `MemoryStats` reports `guard_install_calls` and
`guard_install_bytes` so you can verify guards are actually being installed
in your configuration.

## When to use which

| Need | Policy |
| --- | --- |
| Throughput; no security requirements | `StandardPolicy` |
| Secrets and sensitive buffers that must not leak | `SecurePolicy` |
| Full hardening: corruption resistance plus secrecy | `HardenedPolicy` |

The choice is a type-level parameter, so a subsystem can be moved between
policies in one line and the rest of the codebase is unaffected.
