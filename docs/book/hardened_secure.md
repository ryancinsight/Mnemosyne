# Hardened and Secure Policies

Mnemosyne provides compile-time policy choices for security-sensitive or
adversarial contexts. `StandardPolicy`, `SecurePolicy`, and `HardenedPolicy`
are zero-sized types; their associated constants specialize the allocator
without storing a runtime mode flag. The full policy matrix and usage surface
are in the [allocation policy chapter](alloc_policies.md).

## `SecurePolicy`

`SecurePolicy` enables three implemented mitigations at compile time:

| Mitigation | Effect |
|-----------|--------|
| `ZERO_INITIALIZE = true` | Newly allocated or exposed bytes are zeroed before use |
| `ENABLE_POISONING = true` | Freed bytes are overwritten with `0xDE` |
| `RANDOMIZE_ALLOCATION = true` | Each initialized page receives a seeded free-list permutation |

Zero-initialization prevents information leakage through newly exposed bytes.
Poisoning makes stale or use-after-free observations more diagnosable when the
block has not yet been reused; it does not make use-after-free valid. The page
permutation makes allocation order less predictable, raising the cost of
heap-layout exploitation without adding a random-number call to every
allocation.

## `HardenedPolicy`

`HardenedPolicy` adds free-list pointer encryption to `SecurePolicy`:

| Mitigation | Effect |
|-----------|--------|
| `ENABLE_FREE_LIST_ENCRYPTION = true` | XOR-encodes free-list links with a per-page cookie |

The cookie is derived from the page's segment state. It makes a corrupted
free-list link fail to decode to the expected aligned address instead of
exposing a plain pointer representation, raising the bar for free-list
corruption and unlinking attacks.

## Active mitigation masks

`P::MITIGATION_FLAGS` advertises the implemented data-plane mitigations. The
mask is compile-time metadata, not a runtime feature switch:

| Policy | Active mask | Meaning |
| --- | ---: | --- |
| `StandardPolicy` | `0x00` | no policy-specific mitigation |
| `SecurePolicy` | `0x0B` | poisoning, zero initialization, randomized page order |
| `HardenedPolicy` | `0x0F` | secure policy plus free-list encryption |

The registry also names reserved hooks for page-wake hysteresis, free canaries,
and sized-free validation. Those bits are not advertised until their
end-to-end data paths are implemented.

## Performance Trade-Offs

Each mitigation has a measurable cost:

| Mitigation | Cost |
|-----------|------|
| Zero-init | Memory write traffic for each newly exposed range |
| Poisoning | Memory write traffic on each free; allocation poisoning applies only when zero-init is disabled |
| Randomize | One seeded page free-list permutation when a page is initialized |
| Encryption | One XOR on each free-list link encode or decode |

The standard policy keeps four segments as the `purge_lazy` warm threshold;
secure and hardened policies inherit a zero threshold. This is purge guidance,
not an eager-retention rule. Use `StandardPolicy` when its threat model is
acceptable and allocation latency or write traffic is the binding constraint;
select `SecurePolicy` or `HardenedPolicy` at the allocator type boundary when
the corresponding safety properties are required.
