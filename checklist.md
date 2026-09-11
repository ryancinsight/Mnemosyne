# Checklist

## MN-WASM-ENV-2026-09-11 (owner: codex)

- [ ] Gate the environment reader away from host-only `getenv` on wasm32.
- [ ] Run native and wasm32 strict checks for `mnemosyne-local`.
- [ ] Rebuild RITK's `ritk-snap` cdylib for wasm32 and record the result.
- [ ] Run the focused native allocator tests, then close the item with the
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
