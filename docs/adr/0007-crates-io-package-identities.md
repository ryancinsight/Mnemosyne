# ADR 0007: crates.io package identities

Status: Accepted

## Context

The `mnemosyne` and `mnemosyne-core` crates.io packages are owned by
unrelated projects. The local facade cannot publish version 0.6.0, and the
existing registry package `mnemosyne-core 0.2.0` is not this repository's
implementation.

## Decision

Publish the facade as `mnemosyne-memory` and the core as
`mnemosyne-memory-core`. Preserve the Rust library names `mnemosyne` and
`mnemosyne_core`. Dependencies retain their existing Rust-facing keys and
select the new Cargo package identities with `package = ...`.

Publish `mnemosyne-build-util` because `mnemosyne-prof` needs it while
verifying a registry archive. Do not publish `mnemosyne-benchmarks`; it is a
measurement harness, not a reusable library.

## Alternatives

Ownership transfer depends on unrelated owners and does not provide a bounded
release path. Resolving the occupied registry packages would compile unrelated
code and is rejected.

## Verification

Clean-checkout package verification must resolve every internal dependency by
the new package identity while compiled Rust paths remain unchanged. The
workspace nextest suite verifies the unchanged allocator behavior.
