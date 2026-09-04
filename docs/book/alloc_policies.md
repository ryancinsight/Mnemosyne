# Allocation Policies

Mnemosyne selects allocation behavior through the sealed `AllocPolicy` trait.
Each built-in policy is a zero-sized type (ZST), and its flags are associated
constants. A generic allocation path therefore specializes once per policy;
the standard path has no runtime security-mode branch.

## Built-in policies

| Policy | Allocation initialization | Free-time action | Free-list order | Link encoding | Warm-segment guidance |
| --- | --- | --- | --- | --- | ---: |
| `StandardPolicy` | None | None | Sequential | Plain | 4 |
| `SecurePolicy` | Zero-fill | Poison | Randomized | Plain | 0 |
| `HardenedPolicy` | Zero-fill | Poison | Randomized | XOR-encrypted | 0 |

`ENABLE_POISONING` controls free-time poisoning and the allocation pattern when
`ZERO_INITIALIZE` is false. For the secure and hardened policies,
`ZERO_INITIALIZE` takes precedence, so newly allocated bytes are zeroed rather
than filled with `POISON_ALLOC_BYTE`. Reallocation applies the same policy to a
newly exposed range and poisons a truncated range when poisoning is enabled.

The active mitigation mask is available through `P::MITIGATION_FLAGS`:

| Policy | Active mask |
| --- | ---: |
| `StandardPolicy` | `0x00` |
| `SecurePolicy` | `0x0B` |
| `HardenedPolicy` | `0x0F` |

The mask reports only mitigations with an end-to-end runtime implementation:
poisoning, zero initialization, free-list encryption, and randomized page
allocation. The registry's `mitigations::ALL` also names reserved hooks for
page-wake hysteresis, free canaries, and sized-free validation; those bits are
not advertised by a built-in policy until their data-plane paths are complete.
`GWP_SAMPLE_RATE` is zero for the built-in policies, so guard-page sampling is
not active.

## Performance and memory tradeoffs

`StandardPolicy` avoids policy-specific writes on the allocation and free hot
paths. `SecurePolicy` and `HardenedPolicy` add memory traffic for zeroing and
poisoning; `HardenedPolicy` also adds the XOR encode/decode work for free-list
links and the one-time randomized page-list construction. Those costs buy
different safety properties and are selected at the allocator boundary rather
than paid by standard-policy callers.

`SEGMENT_POOL_WARM_THRESHOLD` is caller guidance for [`purge_lazy`], not an
automatic retention policy. Passing `StandardPolicy::SEGMENT_POOL_WARM_THRESHOLD`
keeps up to four committed segments available for a burst after purging. Use
`purge` when all retained segments must be released, or `reset` when the
virtual ranges should stay reusable while their physical backing is dropped.

[`purge_lazy`]: https://docs.rs/mnemosyne/latest/mnemosyne/fn.purge_lazy.html

## Usage

The default `Mnemosyne` unit struct is the standard CPU allocator:

```rust,ignore
use mnemosyne::Mnemosyne;

#[global_allocator]
static ALLOC: Mnemosyne = Mnemosyne;
```

To select a policy explicitly, use the generic allocator. Its backend defaults
to `MemoryBackendWrapper`:

```rust,ignore
use mnemosyne::{MnemosyneAllocator, SecurePolicy};

#[global_allocator]
static ALLOC: MnemosyneAllocator<SecurePolicy> = MnemosyneAllocator::new();
```

The generic allocator keeps policy and memory-backend variation separate. A
compatible backend is selected through `B` without adding virtual dispatch to
allocation or free operations.

The runnable [allocator statistics example](examples/alloc_policies.md)
prints the compile-time policy metadata and measures live allocations and
mapped bytes using the standard policy.
