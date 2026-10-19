# Checklist

Target for this slice: arena 0.7.1 / memory 0.10.1.

## MN-EUNOMIA-011-RANGE [patch]

- [x] Recheck current defaults, existing PR ownership, instructions and active
      required checks; reuse this isolated draft branch.
- [x] Preserve all Rust/test/workflow bytes and committed external lock records;
      retain the existing compatible patch targets and 0.10 provider lock.
- [x] Validate both actual canonical provider sources [0.10 source](https://github.com/ryancinsight/eunomia/commit/a231e54dd3311bc916e78c91d994b00d651c1646) and [0.11 source](https://github.com/ryancinsight/eunomia/commit/22daa6c67e3d0f16d346a312b6835ccbf351a708):
      483 Eunomia-enabled workspace tests plus 151 all-feature arena tests, zero skips. Activate the optional
      Eunomia boundary where applicable; keep NumPy/PyO3 migration claims separate.
- [x] Pass 0.11 strict lint, warning-denied docs/doctests and applicable minimal builds.
- [x] Pass reviewed-source MSRV, strict patch API and standard package verification.
- [x] Reproduce old-internal normalized-package trait/type failures against0.11.
- [x] Raise affected internal minimums and pass actual normalized-package value
      consumers on0.10/0.11; reject old explicit internal pins at resolution.
- [ ] Verify clean final-head package source identity after the status update.
- [ ] Pass unchanged required hosted checks, including Miri, and independent review.
- [ ] Merge normally; verify automatic publication, registry checksum/VCS/source
      contents and any selected documentation deployment.
- [ ] Revalidate canonical downstream resolution on the published 0.11 provider
      before closing this coordinated prerequisite.


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
- [x] Repair native optional-guard geometry; preserve the original telemetry and
      mapping/release assertions. Default 481, individual guards149 each, and
      all-feature arena 151 pass, zero skips. Windows/Linux cross checks pass.
- [x] Obtain authorization for the existing required Miri CI without changing gates.
- [x] Collect all exact-head required CI results at f36febc8: Miri, Loom,
      ThreadSanitizer, native verification and CI aggregate all pass.
- [x] Select Eunomia 0.10.0 published source a231e54d with verified registry receipts.
- [x] Merge PR #237 normally as b7e0e1e with an identical reviewed source tree;
      publisher 38752721843 succeeds. All six registry archive checksums and VCS
      source identities are independently verified, not yanked (ADR 0013).

## MN-NATIVE-GUARD-GEOMETRY [patch]

- [x] Check current owner file/hunk metadata without reading held implementation.
- [x] Reproduce the exact failing combined-feature case on unchanged main.
- [x] Record real OS page size and tail/header/both confirmed-call/byte matrix.
- [x] Preserve the original telemetry/mapping/release assertions and native deadline.
- [x] Repair only optional native guard geometry outside held registry hunks;
      validate real 16-KiB outcomes, metadata exclusion and full slack decommit.
- [x] Collect final Linux guard matrix, required Miri/Loom/TSan/CI and blocking
      API/package acceptance before publication. Windows runtime is unverified;
      cross compilation does not establish native execution.


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
