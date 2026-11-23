# Checklist

## Stable release checklist

Basis `2326baa3ff15`, representative manifest `mnemosyne-memory 0.10.2`. Target 1.0.0. [Backlog item](backlog.md#mnemosyne-stable-qualification).

- [ ] [patch] Triage MN-RESCUE-QUEUE one hunk at a time against the 0.10.2 memory and 0.7.2 arena source. Retain a reproducing input for each live defect and a source reference for each obsolete change.
- [ ] [patch] Verify allocation layout, alignment and reclamation across the local allocator, arena and facade. Run the owned safety and performance gates on real allocations and a downstream Moirai workload.
- [ ] [major] Close M-policies-backends and the facade API review, including the support status of independently versioned packages and the registry-only memory consumer.
- [ ] [minor] **0.11.0.** Classify each rescue-queue change against a reproducible allocation or release defect. Check live allocation counts, alignment and reclamation through the real allocator under contention. [Source and baseline](gap_audit.md#incremental-release-baseline).
- [ ] [minor] **0.12.0.** Exercise policy and backend selection under the same allocation pattern. Compare peak live bytes and reclamation, and record unsupported host features as explicit capability outcomes.
- [ ] [major] **1.0.0-beta.1 → 1.0.0-rc.1 → 1.0.0.** Freeze the supported surface at beta. A fresh allocation consumer respects alignment, lifetime and reclamation guarantees on the supported platforms. Repeat the applicable exits on the exact RC before stable promotion. Run the [release scope](docs/adr/0014-release-scope-1-0.md) exits and attach reviewed, exact-revision evidence for this member’s [Atlas qualification criteria](https://github.com/ryancinsight/atlas/blob/17b0124a7548ad81e817284afc5cb115c1d1bc78/docs/parity/repositories.json).

Active target: Arena 0.7.2 and Memory 0.10.2 [patch].

- [ ] [patch] Verify Eunomia 0.10, 0.11, and 0.12 scratch compatibility.
- [ ] [patch] Verify the package archives, SemVer, native CI, and fresh registry resolution.

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
