# Backlog

## Ready

<a id="mn-eunomia-010-cohort"></a>
- [ ] [major] **MN-EUNOMIA-010-COHORT — adopt the current datatype provider.**
  priority: verification; status: blocked; owner: coordinated Coeus release integrator.
  **Scope:** manifest/lock versions, the native allocation-counting test's
  synchronization, a feature-gated macro declaration, a platform-specific doc
  link, changelog, ADR 0013 and this checklist.
  **Outcome:** Eunomia 0.10 uses one canonical provider identity; arena/local/heap
  advance to 0.7.0 and the memory facade to 0.10.0 for public marker/pool trait
  identities. Decay and the C shim use 0.3.3 dependency patches; the C ABI stays
  unchanged. Unaffected core/backend/profiler/hardening/build-util versions stay.
  **Acceptance:** ordinary native, strict lint, docs, package/API and exact-head
  required CI gates; preserve production behavior, fixtures, test payloads,
  assertions and timeouts.
  **Evidence:** all 481 default native cases pass, zero skips, after replacing
  lazy-allocating Barrier waits in the isolation test with atomic coordination.
  Strict all-target/all-feature Clippy, warning-denied docs, 26 doctests (one
  existing ignore), six standard packages and locked-source API classification
  pass. The four minor boundaries skip major-sensitive API rules by design;
  the two dependency-only patches each compare 196 rules.
  **Native blocker:** the optional Eunomia/both-guard suite passes 147/148 cases.
  Its telemetry case fails identically on unchanged macOS main. On 16-KiB OS
  pages, tail-only confirms one 4096-byte request; header-only confirms none;
  both confirm one 4096-byte request while the assertion expects 8192 bytes.
  The fixed 4-KiB request geometry also violates the backend's documented
  system-page address/length preconditions. Correcting only the expected
  counter would leave that contract issue unresolved. Keep the assertion and
  production guard code intact pending provider ownership/scope resolution.
  **Required CI:** the user authorizes the existing required Miri checks.
  Their actual exact-head results remain pending; no gate, filter or timeout
  changes are permitted. Package publication remains outside this resumed turn.
  **Decision:** [ADR 0013](docs/adr/0013-eunomia-010-cohort.md). Existing held
  provenance, sampler and performance owner branches remain untouched.

<a id="mn-native-guard-geometry"></a>
## MN-NATIVE-GUARD-GEOMETRY — reconcile native guard request geometry [patch]

- Status: blocked; priority: correctness; owner: allocator provider coordination.
- Finding: `fresh_segment_install_increments_guard_telemetry_and_round_trips`
  fails on unchanged main and the datatype candidate with both guard features.
  Native macOS has 16384-byte pages. Header offset `65536 - 4096 = 61440`
  has page remainder 12288; both requests use 4096-byte lengths. These do not
  meet `MemoryBackend::make_guard`'s system-page preconditions. The backend
  records confirmed outcomes only; observed combined delta is one call/4096
  bytes, compared with the test's 8192-byte expectation. Header-only's test
  passes with zero installs because its counter assertion is conditional.
- Scope: establish a backend/arena page-geometry contract before changing
  protection behavior. No test-ignore, expected-count-only repair, platform
  exclusion, safety-contract relaxation or timeout change constitutes closure.
- Ownership: active held PR #218 changes mapping/registration in the same
  `map_fresh_segment` function, outside the guard hunks; PR #190 also touches
  the test file outside this case. Only file/hunk metadata was compared.
  Neither owner branch nor its implementation is changed or resumed.
- Acceptance: preserve guard-address tests and mapping/release assertions;
  validate real native guard outcomes and telemetry on 4-KiB and 16-KiB page
  hosts, plus all applicable feature/native/lint/docs gates. Reclassify the
  version if the repair requires a public backend API change.
- Evidence: the native tail/header/both matrix reproduces 1/4096, 0/0 and
  1/4096 calls/bytes respectively. The combined case fails; the unchanged-main
  reproduction fails at the same assertion. Diagnostic output is retained
  outside the repository; test and production source remain unchanged.


<a id="MN-RESCUE-QUEUE"></a>
- [ ] [correctness] **MN-RESCUE-QUEUE — complete or close the open rescue PR.**
  priority: correctness; status: todo; needs: none; scope: the head ref of the
  PR below (`crates/`, `fuzz/`, `README.md`, `backlog.md`, `checklist.md`).
  **Outcome:** each listed rescue PR is completed onto current main (ported,
  verified, merged) or closed once its diff resolves empty against main.
  **Acceptance:** no open `rescue/` PR for mnemosyne remains unaccounted for.
  **Next step:** port the head onto current main (it is based on 76a32552e,
  107 commits behind) and resolve its diff hunk by hunk against main.
  - ryancinsight/mnemosyne#190 (`rescue/mnemosyne-audit-20260928`): one commit,
    56 files, +1727/-1684; mostly behavior-preserving splits (tests extracted
    from `segment/`, `tagged_stack`, `fuzz/op_sequence`; `mnemosyne-prof` and
    `mnemosyne-decay` control/hook modules; `segment_acquisition.rs` out of
    `routing/cold.rs`), plus backend and TLS edits and deletion of
    `backlog.md`/`checklist.md` content (stale against current main).

<a id="MN-LOCAL-MIRI-UB"></a>
- [ ] [correctness] **MN-LOCAL-MIRI-UB — remove the undefined behavior Miri reports in `mnemosyne-local`.**
  priority: correctness; status: todo; needs: none; scope:
  `crates/mnemosyne-local/src/free/classified.rs`,
  `crates/mnemosyne-local/src/local_alloc/page/allocation.rs`,
  `crates/mnemosyne/tests/global_alloc_tests/`, `.github/workflows/ci.yml`.
  **Outcome:** any program that allocates through `Mnemosyne` is Miri-clean. Stacked Borrows rejects the page-metadata read at `free/classified.rs:58`; Tree Borrows rejects it at `free/classified.rs:101` and the `alloc_count` update at `local_alloc/page/allocation.rs:57`. Both reproduce on the Windows host and on `x86_64-unknown-linux-gnu`, and the `-p mnemosyne-local` unit-test job does not reach them.
  **Reproduce:** `MIRIFLAGS="-Zmiri-disable-isolation [-Zmiri-tree-borrows]" cargo +nightly miri test -p mnemosyne-memory --test global_alloc_tests -- basic::test_basic_allocation`.
  **Acceptance:** Miri clean on `global_alloc_tests` under Stacked Borrows and Tree Borrows, both added to the CI `miri` job; then `counting_allocator` drops its `cfg(miri)` `System` inner and wraps `Mnemosyne` under Miri too.
  **Next step:** read the borrow-stack report at each of the three sites for the retag that invalidated the tag, then derive the access from the segment, as the reclamation seam does (MN-437).
  basis: origin/main 66a5a86899cb73fd8296b4a6858351bc3baf0398.

<a id="MN-PROF-MIRI-FRAMES"></a>
- [ ] [correctness] **MN-PROF-MIRI-FRAMES — keep captured frame provenance in `mnemosyne-prof`.**
  priority: correctness; status: todo; needs: MN-LOCAL-MIRI-UB; scope:
  `crates/mnemosyne-prof/src/sampler/`, `.github/workflows/ci.yml`.
  **Outcome:** the sampler stores captured frames as pointers rather than `usize`,
  so `backtrace::resolve` receives the provenance `frame.ip()` carried. With the
  MN-LOCAL-MIRI-UB fix applied (PR #210),
  `leak::test_leak_detector_integration` reaches Undefined Behavior under Miri
  (Stacked and Tree Borrows) in `backtrace`'s `miri_resolve_frame`, called from
  `sampler/report.rs:133` with an address that has no provenance.
  **Acceptance:** `global_alloc_tests` Miri-clean under both models with no test
  filter, and the CI facade Miri steps drop `not test(/^leak::/)`. That run also
  guards the sampler's TLS copy-out fix, which only `leak::` reaches under Miri.
  **Next step:** replace the interner's `[usize]` frame type with a pointer
  newtype, keeping its hash and the shard selection unchanged.

<a id="MN-WASM-ENV-2026-09-11"></a>
- [x] [patch] **MN-WASM-ENV-2026-09-11 — make allocator option discovery link-safe on WASM.**
  status=done; integrator=root; branch=`fix/mnemosyne-wasm-env-options-001`;
  latest=`870854f`; last-update=2026-09-11.
  **Outcome:** `mnemosyne-local` does not reference host `getenv` when compiled
  for `wasm32-unknown-unknown`; explicit `configure` remains the supported
  runtime configuration path. **Acceptance:** native and WASM warning-denied
  checks pass, the RITK `ritk-snap` cdylib links for WASM, and native allocator
  tests retain their environment-backed behavior. **Dependency:**
  [MN-WASM-2026-09-06](backlog.md#MN-WASM-2026-09-06). Native workspace
  nextest passes 459/459; all-features remains host-environment blocked by the
  existing MSYS2 GNU jemalloc archive on the MSVC target.

<a id="MN-WASM-2026-09-06"></a>
- [x] [arch] [minor] **MN-WASM-2026-09-06 — provide a portable WebAssembly memory backend.**
  status=done; integrator=root; branch=`codex/mnemosyne-wasm-pointer-width`;
  outcome delivered by PR #141; closure retained by PR #179.

<a id="mn-em-book-depth-1"></a>
- [x] **MNEM-BOOK-DEPTH-1** [docs][minor] status=done owner=codex
  branch=`perf/mnemosyne-scratch-release`; latest=`af7a23a`.
  Outcome: corrected the full book's implementation contracts, examples, and stack ownership; `mdbook test` and `mdbook build` pass.

### MN-SCRATCH-RELEASE-2026-09-04 — Pooled scratch had no reclamation path [minor] [perf] — done <a id="mn-scratch-release-2026-09-04"></a>

- **Closed 2026-09-22.** `ScratchPool::release`, `ScratchPool::reset`, and the
  `ScratchBank` pass-throughs are all shipped to main and documented in the
  CHANGELOG. Provisions are recorded by `with_scratch_bounded`
  (`borrow_slot<PROVISION=true>`) and honoured by `release`. Zero-allocation
  warm pass confirmed in the CHANGELOG acceptance note.

### MN-SCRATCH-GROWTH-COST-2026-09-04 [patch] [perf] — done <a id="mn-scratch-growth-cost-2026-09-04"></a>

- **Outcome:** Preserve geometric scratch growth while `release` reclaims
  capacity above each recorded provision, avoiding a reallocation regression
  in the retention fix.
- **Scope:** `mnemosyne-arena` aligned scratch storage, focused scratch tests,
  and synchronized changelog/backlog text on PR #127.
- **Acceptance:** growth retains its overflow-safe doubling policy and remains
  amortized; release retains the requested provision exactly; a regression test
  bounds growth events;
  format, strict Clippy, Nextest, and Miri pass.
- **Rejected 2026-09-23:** an exact-growth variant for the bounded path
  (`ensure_len_exact`, from a stranded local series) contradicts this
  acceptance -- it drops amortized doubling -- and main already trims to the
  provision in `release`. Remaining: the growth-events regression test.
- **Risk / delivery:** `[patch]` private growth policy and regression coverage;
  integrator current Atlas session; branch `perf/scratch-release`.

<a id="mn-459"></a>
- [x] [patch] **MN-459 — bring `mnemosyne-heap` under the Miri gate.**
  status=done; integrator=codex; last-update=2026-09-22.
  Both Stacked Borrows and Tree Borrows Miri jobs for `mnemosyne-heap` are
  active in `.github/workflows/ci.yml` (confirmed in the CI `miri` job).
  Close confirmed by CI comment: "Heap joins under MN-459 after its own test
  helpers pass both borrow models."

<a id="mnem-unsafe-doc-1"></a>
- [x] **MNEM-UNSAFE-DOC-1** [verification][patch] status=done owner=Claude
  **Closed 2026-09-22.** Safety ratchet (`scripts/safety_comment_scan.py check`)
  reports baseline **0** (from an original 84). All 742 production `unsafe {}`
  blocks carry a `// SAFETY:` comment. The CI `SAFETY comment ratchet` step
  enforces this invariant going forward.

## Blocked

<a id="atlas-mnemosyne-stage-d1"></a>
- [ ] [minor] **ATLAS-MNEMOSYNE-STAGE-D1 — branded device buffers.**
  status=blocked; owner=external integration; last-update=2026-09-04.
  Blocker: the Mnemosyne `MemoryBackend`, Melinoe lifetime brands, and
  Hephaestus device/stream completion contract do not yet share a branded
  asynchronous ownership boundary. Re-open when that provider seam and a
  concrete Coeus consumer target exist; no downstream adapter.
<a id="mnem-provider-publish-1"></a>
- [ ] [patch] **MNEM-PROVIDER-PUBLISH-1 — publish provider crates.**
  status=blocked; owner=external; last-update=2026-09-04. Re-open when
  crates.io contains the required Eunomia and Melinoe versions and Themis has
  a released source-aligned dependency graph.
<a id="mnem-tagged-pool-pack-1"></a>
- [ ] [perf-experiment] **MNEM-TAGGED-POOL-PACK-1 — compare packed pool state.**
  status=blocked; owner=codex; last-update=2026-09-04. Re-open on a coherent
  native Windows toolchain and a quiet Criterion run; compare warm-pool,
  handoff, eviction, and retention rows before changing cache-line layout.
<a id="ar-4"></a>
- [ ] [patch] **AR-4 — strengthen benchmark gate statistics.**
  status=blocked; owner=codex; last-update=2026-09-04. Re-open on a quiet
  machine for the paired sampling change and threshold baseline refresh.

