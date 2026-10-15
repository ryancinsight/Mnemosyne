# Checklist

## MN-EUNOMIA-010-COHORT [major] — current slice

- [x] Inspect current main, open owner PRs and reserved ADR numbers; isolate work.
- [x] Record the datatype/public pool boundary and align the affected versions.
- [x] Reproduce the native counting-test failure on unchanged main; preserve
      its payload/assertions and pass the focused atomic-coordination regression.
- [x] Pass all 481 default native cases with zero skips and the original deadline.
- [x] Pass strict all-target/all-feature Clippy and warning-denied docs;
      26 doctests pass with one existing ignore.
- [x] Verify all six standard package archives and locked-source API release
      classification; minor boundaries do not claim backward compatibility.
- [ ] Resolve the optional guard telemetry case: 147/148 pass; the same failure
      reproduces on unchanged macOS main. Assertions and guard code stay intact.
      See MN-NATIVE-GUARD-GEOMETRY: native 16-KiB pages produce one confirmed
      4096-byte request, and fixed guard requests violate documented page
      preconditions. An expected-count-only change does not resolve that issue.
- [x] Obtain authorization for the existing required Miri CI without changing gates.
- [ ] Collect all exact-head required CI results, including Miri.
- [x] Select Eunomia 0.10.0 published source a231e54d with verified registry receipts.
- [ ] Verify this cohort's package registry receipts after required integration.

## MN-NATIVE-GUARD-GEOMETRY [patch]

- [x] Check current owner file/hunk metadata without reading held implementation.
- [x] Reproduce the exact failing combined-feature case on unchanged main.
- [x] Record real OS page size and tail/header/both confirmed-call/byte matrix.
- [x] Preserve the original assertions, guard implementation and native deadline.
- [ ] Coordinate the backend/arena page contract and repair protection geometry
      separately from held registry/Miri work; validate 4-KiB and 16-KiB hosts.


## MN-WASM-ENV-2026-09-11 (owner: codex)

- [x] Gate the environment reader away from host-only `getenv` on wasm32.
- [x] Run native and wasm32 strict checks for `mnemosyne-local`.
- [x] Rebuild RITK's `ritk-snap` cdylib for wasm32 and record the result.
- [x] Run the focused native allocator tests, then close the item with the
      upstream and consumer commit references.

## MN-SCRATCH-RELEASE-2026-09-03 (owner: unclaimed)

- [ ] Measure Apollo's long-lived worker scratch retention and confirm the
      second pass remains allocation-free.
- [ ] Select the consumer-controlled reclamation trigger without adding work
      to the `with_scratch` steady-state path.
- [ ] Verify release behavior with a paired retention/allocation probe.

## MN-436 (owner: codex)

- [x] Reproduce and localize the exposed-provenance warnings through the Leto
      single-write storage path.
- [x] Preserve backend-allocation provenance in segment/page recovery and
      remove integer-to-pointer reconstruction.
- [x] Add boundary/value tests for aligned recovery and allocator-owned drops.
- [x] Run focused strict Miri, nextest, Clippy, docs, and SemVer gates.
- [x] Replace the hosted Miri test's `&mut` page projection with the
      mapping-provenance-preserving raw projection.
- [ ] Collect the hosted full-suite Miri result, then close the lease and board
      item with the merged source commit.

## Blocked — packed tagged pool state (owner: codex)

- [x] Preserve the packed layout assertions and synchronize `take_all`
      documentation.
- [ ] Compare warm-pool, handoff, eviction, and retention Criterion rows on a
      coherent native Windows toolchain; re-open when that environment exists.
