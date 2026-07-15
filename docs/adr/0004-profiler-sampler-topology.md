# ADR 0004: Profiler Sampler Vertical Topology

Status: accepted

## Context

Before this extraction, `mnemosyne-prof/src/sampler.rs` contained stack hashing, stack interning,
active-sample storage, sampling cadence, stack capture, and report rendering.
The mixed ownership makes the sampler's lock, allocation, and output concerns
hard to audit independently and places the file above the repository's deep
vertical hierarchy target.

## Decision

Keep `sampler` as the bounded-context manifest and move each responsibility to
one canonical leaf module:

- `hasher`: the provider-owned deterministic hash implementation;
- `stack_interner`: stack identity, deduplication, and reference lifecycle;
- `capture`: bounded frame capture and sampling interval generation;
- `store`: sharded active-sample ownership and count state;
- `sampling`: allocation/free event orchestration;
- `report`: profile and leak report aggregation and output.

The public `Sample`, `StackId`, `dump_profile`, and `dump_leaks` contracts stay
at the sampler boundary. The extraction is behavior-preserving and keeps the
existing generic-free hot path, lock ownership, allocation behavior, and
`StackId` representation unchanged. No compatibility module or forwarding
wrapper is retained after each leaf moves.

## Rejected alternative

Leaving the sampler as one file preserves short-term call-site stability but
retains mixed responsibility and prevents independent contention/memory
measurement. Duplicating stack or report helpers in consumers would violate
the provider-owned SSOT boundary.

## Verification

Each increment runs `cargo fmt --all -- --check`, warning-denied Clippy, the
focused `mnemosyne-prof` nextest suite, doctests, and rustdoc. The final
extraction additionally runs the workspace consumer tests and compares the
profiler Criterion rows against the pre-extraction baseline. A regression in
allocation latency or sampled value semantics blocks the extraction.
