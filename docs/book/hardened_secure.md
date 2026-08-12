# Hardened and Secure Policies

Mnemosyne provides two hardened allocation policies for use in security-
sensitive or adversarial contexts.

## `SecurePolicy`

`SecurePolicy` enables three mitigations at compile time:

| Mitigation | Effect |
|-----------|--------|
| `ZERO_INITIALIZE = true` | Every allocation is zeroed before being returned |
| `ENABLE_POISONING = true` | Freed memory is overwritten with a poison pattern |
| `RANDOMIZE_ALLOCATION = true` | Slot selection is randomized to resist heap spraying |

Zero-initialization prevents information leakage through uninitialized reads.
Poisoning ensures use-after-free reads see a deterministic garbage pattern
rather than stale data. Randomized allocation raises the cost of heap-layout
exploitation.

## `HardenedPolicy`

`HardenedPolicy` adds free-list pointer encryption to `SecurePolicy`:

| Mitigation | Effect |
|-----------|--------|
| `ENABLE_FREE_LIST_ENCRYPTION = true` | XOR-encrypts free-list pointers with a per-segment secret |

This raises the bar for free-list corruption exploits (heap unlinking attacks).

## Performance Trade-Offs

Each mitigation has a measurable cost:

| Mitigation | Cost |
|-----------|------|
| Zero-init | One `memset` per allocation |
| Poisoning | One `memset` per deallocation |
| Randomize | One RNG call per allocation |
| Encryption | Two XOR ops per free-list access |

Use `StandardPolicy` in production workloads where these trade-offs are
unacceptable and threat modelling does not require them.
