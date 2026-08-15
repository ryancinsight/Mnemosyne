# ADR 0008: Policy-keyed allocator statistics

Status: Accepted

## Context

ADR 0001 bound the free-list encryption mode into TLS slot selection, so every
`(backend, encryption mode)` pair owns an independent `ThreadAllocator` cache.
`LocalAllocatorSelector` exposes that as `with_allocator_for_policy::<P, _>`,
alongside the older policy-blind `with_allocator`, which always reaches the
standard slot.

The statistics surface never followed. `thread_allocator_stats::<B>()` calls
`with_allocator`, and `mnemosyne::memory_stats_generic::<B>()` calls it in turn.
An application that installs `MnemosyneAllocator<HardenedPolicy>` therefore asks
for its allocator's counters and receives the standard slot's — which, since
nothing ever allocated there, reports a near-empty allocator. No error, no
warning: live allocations, owned segments, page refills and size-class occupancy
all read as if the process had barely allocated.

This is a wrong answer rather than a missing feature. Anyone using these numbers
to size a cache, chase a leak, or gate a release under a hardened build is
reasoning about the wrong allocator, and the reported values are plausible
enough not to look broken.

It surfaced while writing MN-447's page-recycling test, whose hardened variant
could not assert a single counter and had to prove recycling by pointer address
instead.

The sibling entry points are *correctly* policy-blind and stay unchanged:
`purge_generic`, `reset_generic` and `decay` act on the per-backend segment pool
and the decay thread, neither of which is keyed by policy.

## Decision

Key the statistics surface by policy, through the same generic parameter the
allocator entry points already take:

- `mnemosyne_local::thread_allocator_stats::<P, B>()` routes through
  `with_allocator_for_policy::<P, _>`.
- `mnemosyne::memory_stats_generic::<P, B>()` gains the same parameter and
  forwards it.
- `mnemosyne::memory_stats()` keeps its signature and supplies `StandardPolicy`.

The convenience/generic pairing then matches the allocator types exactly:
`Mnemosyne` is `MnemosyneAllocator<StandardPolicy>`'s shorthand, and
`memory_stats()` is `memory_stats_generic::<StandardPolicy, _>`'s. A caller who
parameterized their allocator parameterizes their statistics the same way.

Both signature changes are breaking, so this is `[major]`.

## Alternatives

**A second `_for_policy` entry point beside each existing function.** Rejected:
this is the parallel-API defect. Variation that the type system can express
belongs in a generic parameter, not in a name — the same reason
`with_allocator_for_policy` is a poor precedent to extend rather than a pattern
to copy. It would also leave the existing names still returning the wrong
allocator for hardened callers, so the defect would persist under a
better-documented alias.

**Aggregate every policy's counters into one report.** Rejected: the policies
own genuinely separate caches, so summing them answers a question nobody asked
and makes a single-policy process's numbers harder to read, while still not
letting a hardened caller see their own allocator in isolation.

**Record the installed policy in process-global state and consult it.**
Rejected: it reintroduces exactly what ADR 0001 removed. The encryption mode is
a compile-time property of the call, and threading it through global state
would both cost a load on a cold path and make the wrong answer possible again
whenever the global disagreed with the caller's `P`.

## Consequences

Callers of `thread_allocator_stats` and `memory_stats_generic` must name a
policy. In-repo that is one production call site and the tests. External
callers get a compile error naming the missing parameter, not a silent change
of behaviour, which is the right failure for a function that was returning the
wrong allocator.

MN-447's hardened recycling test can assert the same counter deltas as its
standard sibling instead of resting on the address argument alone.

## Verification

`cargo-semver-checks` classifies the change (expected: two breaking signature
changes, matching the `[major]` claim). A test asserts that a hardened-policy
caller observes counters from the hardened allocator — specifically that its
live-allocation count tracks its own allocations and that the standard slot's
does not move — which fails against the previous policy-blind implementation.
