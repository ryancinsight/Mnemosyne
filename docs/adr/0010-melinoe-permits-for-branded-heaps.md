# ADR 0010: Use Melinoe permits for branded heap access

Status: Accepted

Date: 2026-09-03

Board item: [MN-469](../../backlog.md#mn-469--use-melinoe-permits-for-branded-heap-handoff)

## Context

`mnemosyne-heap` owns allocation and reclamation, while `melinoe` owns branded
lifetimes and capability tokens. The branded heap, box, vector, and tiered
routing surfaces previously named `ThreadLocalToken` directly. That prevented
the allocator's branded cells from using Melinoe's `SyncRegionToken`, even
though Melinoe already provides the sealed `ReadPermit`/`WritePermit` seam for
thread-local and thread-portable token families.

The allocator's raw state remains thread-confined and unsynchronized. A
cross-thread handoff must therefore move only branded cell handles and the
region token; allocation, reallocation, and reclamation stay with the owning
heap. The handoff must not add a lock, allocation, copy, or runtime dispatch to
payload access.

## Decision

Use Melinoe's sealed `ReadPermit<'brand>` and `WritePermit<'brand>` bounds at
every branded heap, box, vector, and tiered-routing access point. Keep
`ThreadLocalToken` as the token returned by the existing `scope` constructors,
and add `sync_scope` for a `SyncRegionToken`-branded `Heap`.

`BrandedCell<'brand, T>` is explicitly `Send` when `T: Send` and `Sync` when
`T: Send + Sync`. The unsafe implementations rely on Melinoe's permit
exclusion: shared references require a read permit, and mutable references
require the unique write permit. The `Heap` and `BrandedVec`/`BrandedBox`
owners remain thread-confined; the public scoped handoff returns cells and the
token to the owner before reclamation.

## Alternatives rejected

* Making `mnemosyne-heap` depend on a second token implementation would fork
  the brand vocabulary and violate upstream ownership.
* Making the raw allocator `Sync` or putting a lock around it would change the
  allocator's ownership contract and add contention to allocation paths.
* Adding a compatibility wrapper around the old token signatures would leave
  two branded access models in the public surface. Generic permit bounds update
  the existing methods in place.

## Verification

The acceptance evidence is the existing thread-local conformance suite, the
scoped worker regression that mutates and returns one `BrandedCell` without a
copy, warning-denied Clippy and rustdoc, and the paired thread-local/region
branded-access Criterion measurement. The benchmark times only permit-mediated
payload access; setup, allocation, and reclamation are outside its timed
region.
