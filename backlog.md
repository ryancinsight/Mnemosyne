# Backlog

## Ready

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
- [ ] [correctness] **MN-LOCAL-MIRI-UB — make the free path Miri-clean under both aliasing models and ADR 0009.**
  priority: correctness; status: todo; needs: none; scope: `crates/mnemosyne-local/src/free/`,
  `crates/mnemosyne-local/src/free_helpers.rs`, `crates/mnemosyne-core/src/`, `crates/mnemosyne-arena/src/`.
  **Outcome:** blocks whose caller tag is narrower than the 8-byte free-list link (1–7 byte requests,
  class 0) free Miri-clean under Stacked and Tree Borrows without exposed provenance. PR #210 fixes
  the metadata, back-pointer and hand-out sites through exposed provenance (conflicting with ADR 0009);
  with it, `global_alloc_tests` is Tree Borrows-clean and Stacked Borrows rejects only the link write at
  `free_helpers.rs:69` (`basic::test_basic_allocation`). No in-block link satisfies both models;
  proposed ADR 0012 (#213) costs the options.
  **Acceptance:** `global_alloc_tests` Miri-clean under both models (only `leak::` filtered, until
  MN-PROF-MIRI-FRAMES) and `mnemosyne-memory-core` 36/36 under `-Zmiri-strict-provenance`.
  **Next step:** implement the option the ADR 0012 ruling selects.
  basis: origin/main 06769e4321d78088c5621946e224e32b4b1a2c3d.

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

