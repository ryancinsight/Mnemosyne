# Allocation Policies

Mnemosyne is *policy-parametric*: the security posture of an allocator is a
compile-time zero-sized-type (ZST) parameter rather than a runtime flag.  The
fast path of a `StandardPolicy` allocation is identical machine code to a
non-policy allocator — the policy's const flags are resolved by the compiler,
so there is no branch, no check, and no security tax unless the policy says
there is.

## The `AllocPolicy` trait

```rust
pub trait AllocPolicy: private::Sealed + Send + Sync + 'static {
    const ENABLE_POISONING: bool;
    const ZERO_INITIALIZE: bool;
    const POISON_FREE_BYTE: u8 = 0xDE;
    const POISON_ALLOC_BYTE: u8 = 0xAD;
    const ENABLE_FREE_LIST_ENCRYPTION: bool = false;
    const RANDOMIZE_ALLOCATION: bool = false;
}
```

The trait is sealed: the three built-in policies are the only implementors.
The flags are interpreted across the whole stack — the thread-local
allocator, the arena, and the branded heap all read the same policy type, so
a `HardenedPolicy` heap cannot be silently served by `StandardPolicy` free
lists.

## The three policies

| Policy | Poisoning | Zero-init | Free-list encryption | Randomize allocation |
| --- | ---: | ---: | ---: | ---: |
| `StandardPolicy` | off | off | off | off |
| `SecurePolicy` | on | on | off | on |
| `HardenedPolicy` | on | on | on | on |

### `StandardPolicy` — maximum throughput

```rust
pub struct StandardPolicy;
```

Poisoning and zeroing are disabled.  This is the policy for hot numeric
paths — the default for `Mnemosyne`'s global allocator and the policy the
benchmark suite compares against jemalloc and mimalloc.

### `SecurePolicy` — sensitive buffers

```rust
pub struct SecurePolicy;
```

- **Poisoning** writes `POISON_FREE_BYTE` (0xDE) across a block on free and
  `POISON_ALLOC_BYTE` (0xAD) across it on allocation, so stale reads are
  detectable and uninitialized reads are noisy rather than silent.
- **Zero-initialization** writes zeroes on every allocation, for buffers that
  must not leak previous contents.
- **Randomized allocation** perturbs the order in which blocks are handed
  out, frustrating deterministic layout exploits.

### `HardenedPolicy` — full hardening

```rust
pub struct HardenedPolicy;
```

Everything `SecurePolicy` does, plus **free-list encryption**: free-list
pointers are stored in an obfuscated, per-segment-keyed form so that
corrupting a freed block's first word does not yield an arbitrary write
primitive.  Because the thread-local caches key their pages by backend and
free-list encryption mode, a `HardenedPolicy` chain can never be spliced into
a `StandardPolicy` chain by a corrupted free.

## Choosing a policy

The policy is a *type*, not a value, so callers choose at the call site.  A
branded `Heap<'brand, P, B>` is constructed inside a scoped region that
carries the policy parameter:

```rust
use mnemosyne::{branded_scope, SecurePolicy, StandardPolicy};

// Fast path: no poisoning, no zeroing.
branded_scope::<StandardPolicy, _, _, _>(|heap, _token| {
    // heap: Heap<'_, StandardPolicy, _>
});

// Sensitive buffers: zeroed on allocate and free.
branded_scope::<SecurePolicy, _, _, _>(|heap, _token| {
    // heap: Heap<'_, SecurePolicy, _>
});
```

Because the flags are `const`, switching policies costs zero runtime
overhead: the allocator code is monomorphized per policy and the compiler
dead-code-eliminates the disabled branches.  The same scoped `Heap` API is
used for all three policies, so migrating a subsystem to `SecurePolicy` is a
type change, not a rewrite.

See [Hardened and Secure Policies](hardened_secure.md) for the runtime
behavior, guard pages, and the arena's `make_guard` seam.
