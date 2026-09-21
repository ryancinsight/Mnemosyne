# Backlog

<a id="mn-stale-branch-inventory-2026-09-09"></a>

## MN-STALE-BRANCH-INVENTORY-2026-09-09 — Two pushed branches hold unfiled work [patch] — todo

- **Swept 2026-09-09.** Nine local branches, none mapped to an open item.
  Four were fully merged and four were superseded; both classes are deleted.
  The `fix/mnemosyne-pr126-*` trio was three attempts at one fix, and its
  only content main does not already have is a Miri timeout raised from 60 s
  to 180 s -- which the runtime-budget rule rejects on its own terms, since
  the 60 s bound passes.
- **Two remain, preserved on origin rather than locally.** Neither has an
  open pull request, and both are six days old:
  - `origin/refactor/mnemosyne-free-helpers-split` -- 13 commits, 40 files.
    Its free canary, `reset_bin_stats` and sized-free validation all reached
    main by other routes; what it still holds alone is **purge-and-retry on
    the first OS allocation failure**, which appears nowhere on main. That is
    an allocator recovery path and deserves its own item, not a silent port.
  - `origin/feat/phase10-improvements` -- 21 commits, 43 files, including a
    149-line addition to `mnemosyne/src/stats.rs`. Not assessed beyond its
    shape; the same question applies to each commit.
- **Why filed rather than ported.** Both predate the free-list randomization
  that landed in #137 and touch the same allocator paths, so a rebase is a
  re-derivation, not a merge. Each branch's unique commits are read and
  either re-derived against current main or dropped with the reason recorded.
- **Acceptance:** every commit on both branches is either re-derived onto
  main or recorded here as superseded, and the branches are deleted from
  origin.

<a id="mn-test-lock-poisoning-hides-results"></a>

## MN-TEST-LOCK-POISONING-HIDES-RESULTS — One failing test blanks the rest of the run [patch] — todo

- **Observed 2026-09-09** on PR #137's ThreadSanitizer job: one real failure
  in `test_mixed_policy_free_and_realloc_preserve_segment_encoding` was
  followed by twenty `PoisonError { .. }` failures. Every serialized test
  opens with `TEST_LOCK.lock().expect("local allocator test lock was
  poisoned")`, so the first panic while holding it converts every later test
  into a failure that reports nothing about its own subject.
- **Cost:** triage reads twenty red tests and cannot tell which of them the
  change actually broke. Here the answer was one; the run said twenty-one.
- **Fix:** recover the guard rather than propagate the poison --
  `.unwrap_or_else(PoisonError::into_inner)` at each acquisition, behind one
  helper so the choice is stated once. A poisoned lock means an earlier test
  panicked, not that this test's fixture is unusable: each of these tests
  drains the pools it needs on entry.
- **Non-goals:** changing what the tests assert, or the serialization itself.
- **Acceptance:** a deliberately panicking test leaves the following tests
  reporting their own results, and the suite still runs serialized.

## MN-BIN-STATS-RESET-BOUNDARY-2026-09-04 — `reset_bin_stats` is not a synchronized profiling boundary [minor] [perf] — todo <a id="mn-bin-stats-reset-boundary-2026-09-04"></a>

- **Integrator:** unclaimed; **branch:** none; **lease:** none.
- **Last-update:** 2026-09-04.
- **Finding (review of #128, verified against the code).** `reset_bin_stats()`
  calls `flush_current_thread()` and then zeroes `ALLOC_COUNT`/`DEALLOC_COUNT`.
  That flushes the *calling* thread's TLS batch only, so any other worker
  holding a batch accumulated before the reset flushes it afterward and
  reintroduces pre-reset activity into the fresh counters. Symmetrically, a
  `fetch_add` already in flight on another thread can be lost when the reset's
  `store(0, Relaxed)` lands after it.
- **Outcome:** a reset that is a real boundary — every worker's batch
  coordinated, or each batch tagged with a reset generation so a stale batch is
  discarded rather than added.
- **Scope note.** Telemetry accuracy, not memory safety: the counters are
  profiling output, and no allocation path reads them. That is why this is
  filed rather than fixed inside #128 — the fix is a redesign of the batching
  contract in a subsystem landed hours earlier and still being extended, so it
  belongs to its author with a clear boundary rather than to a reviewer's
  drive-by.
- **Acceptance oracle:** a multi-threaded test where a worker holds a pre-reset
  batch across `reset_bin_stats()` and the post-reset totals exclude it.
- **Risk / change class:** [minor] [perf].

## Ready

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
- [ ] [arch] [minor] **MN-WASM-2026-09-06 — provide a portable WebAssembly memory backend.**
  status=review; integrator=root; branch=`codex/mnemosyne-wasm-pointer-width`;
  last-update=2026-09-10.
  **Outcome:** the core segment key and allocator fallback constants compile on
  32-bit WebAssembly, and `mnemosyne-backend` selects a real page-aligned
  global-allocator backend instead of inheriting a host-only default.
  **Acceptance:** pointer-width-safe key derivation; WASM backend allocates and
  deallocates through `Layout`; native and WASM warning-denied Clippy, full
  native tests, and WASM compile pass. **Evidence:** clean branch
  `fix/mnemosyne-wasm-backend` from `origin/main` passes the segment-key
  regression, `cargo check --target wasm32-unknown-unknown --offline`, both
  all-target Clippy runs, and `cargo nextest run --offline` (369/369).
  Pointer-width-safe synthetic fixtures cover wasm32. The shared main checkout
  retains unrelated peer WIP; the provider browser/DICOM consumer remains
  external to this item. RITK's consumer build exposed one remaining
  pointer-width hash literal in `Page::prefer_secondary_free`; this increment
  derives the multiplier per target width. Native nextest (40/40), native
  warning-denied Clippy, and WASM warning-denied Clippy/check pass on the
  current branch.

## In progress

<a id="mn-em-book-depth-1"></a>
- [x] **MNEM-BOOK-DEPTH-1** [docs][minor] status=done owner=codex
  branch=`perf/mnemosyne-scratch-release`; latest=`af7a23a`.
  Outcome: corrected the full book's implementation contracts, examples, and stack ownership; `mdbook test` and `mdbook build` pass.

### MN-SCRATCH-RELEASE-2026-09-04 — Pooled scratch had no reclamation path [minor] [perf] — in progress <a id="mn-scratch-release-2026-09-04"></a>

- **Integrator:** atlas-session; **branch:** `perf/mnemosyne-scratch-release`;
  **lease:** `crates/mnemosyne-arena/src/scratch/{pool.rs,bank.rs,tests.rs}`.
- **Last-update:** 2026-09-04.
- **Outcome:** `ScratchPool::release` and `ScratchBank::release`, so a consumer
  can return pooled scratch at a quiescent point instead of holding each slot's
  high-water mark for the life of the thread.
- **Why, measured downstream.** Apollo's `worker_scratch_retention` probe drives
  transforms through its executor and reads the allocator ledger while the
  workers are still alive: 24 workers retain about **7.2 MB** of scratch after
  the first parallel forward, and the warm pass allocates **nothing**. So reuse
  is working exactly as designed — the cost is pure retention, not churn. The
  storage is this crate's: apollo reaches it through
  `ScratchBank<Complex64, 4>`, and `ScratchBank<T, N>` is `[ScratchPool<T>; N]`,
  so a worker holds up to sixteen `AlignedVec` buffers. Before this there was no
  shrink, clear, or release on the surface at all, and `AlignedVec`'s shrinking
  resize keeps its allocation deliberately, so a slot freed only at thread exit
  — which for a long-lived worker is never.
- **Deliberately not eager.** Releasing on `with_scratch` exit would reintroduce
  the allocation churn the pool exists to remove; the zero-allocation warm pass
  is the property to preserve. Reclamation is a call the consumer makes at a
  moment it chooses, never on the hot path.
- **Soundness.** Both refuse and free nothing while any borrow is live —
  freeing a slot the closure still holds would invalidate its slice — and the
  bank check is all-or-nothing so a caller inside a `with_scratch` closure
  cannot half-release the bank underneath itself. Covered by a test that calls
  `release` from *inside* a live borrow and then keeps using the slice, so the
  guard is proven load-bearing rather than assumed. Miri: 33/33 scratch tests.
- **Acceptance oracle:** apollo's warm-pass window still reports zero
  allocations in both ledgers, and retained scratch after a release falls below
  the ~7.2 MB measured there.
- **Remaining, not addressed here.** The trigger is the consumer's to choose,
  and `AlignedVec::ensure_len` still grows to `min_len.max(capacity * 2)`, so a
  slot can retain an overshoot above the size ever requested — 17,408 elements
  against a 16,384 request in the apollo measurement. Bounding that is
  independent of reclamation and cheaper.
- **Risk / change class:** [minor] [perf]; additive API, no existing path
  changes behaviour.

### MN-SCRATCH-GROWTH-COST-2026-09-04 [patch] [perf] — in-progress <a id="mn-scratch-growth-cost-2026-09-04"></a>

- **Outcome:** Preserve geometric scratch growth while `release` reclaims
  capacity above each recorded provision, avoiding a reallocation regression
  in the retention fix.
- **Scope:** `mnemosyne-arena` aligned scratch storage, focused scratch tests,
  and synchronized changelog/backlog text on PR #127.
- **Acceptance:** growth retains its overflow-safe doubling policy and remains
  amortized; release retains the requested provision exactly; a regression test
  bounds growth events;
  format, strict Clippy, Nextest, and Miri pass.
- **Risk / delivery:** `[patch]` private growth policy and regression coverage;
  integrator current Atlas session; branch `perf/scratch-release`.

<a id="mn-459"></a>
- [ ] [patch] **MN-459 — bring `mnemosyne-heap` under the Miri gate.**
  status=review; integrator=codex; branch=`perf/mnemosyne-scratch-release`;
  last-update=2026-09-04; latest=`a582256`.
  The heap helpers are corrected at their causes:
  the NUMA page probes stay in-bounds, the storage shrink checks avoid
  provenance-invalid metadata recovery under Miri, and both Stacked Borrows and
  Tree Borrows jobs cover `mnemosyne-heap`. The CUDA `dlopen` platform-boundary
  test is now explicit under Miri while native CUDA coverage remains intact;
  close after the hosted full-suite Miri conclusion is green.

<a id="mnem-unsafe-doc-1"></a>
- [ ] **MNEM-UNSAFE-DOC-1** [verification][patch] status=in-progress owner=Claude
  scope=the 84 sites enumerated in `gap_audit.md`; largest clusters
  `mnemosyne-local/src/free.rs` (17), `local_alloc/page/transitions.rs` (11),
  `alloc.rs` (8), `mnemosyne-decay/src/lib.rs` (7),
  `mnemosyne-local/src/realloc.rs` (6), `local_alloc/routing.rs` (6).
  Non-goals: changing any unsafe operation; adding blanket comments that
  restate the code. **Outcome:** every production `unsafe {}` block is
  preceded by a safety comment discharging its specific obligation. 84 of 742
  production blocks (11%) have no `// SAFETY:`/`// Safety:` within 14 lines.
  **Acceptance oracle:** re-running the audit's scan reports 0, and the
  comment at each site names the invariant relied on rather than repeating the
  call. Run as a non-increasing ratchet, module by module, so the count only
  decreases. Note that the tree mixes `// SAFETY:` and `// Safety:` — pick one
  (terminology SSOT) and normalize in the same pass so the scan can be
  mechanized as a CI check. **Dependencies:** none. **Risk/change class:**
  [patch]. **Effort:** L.
  **Ratchet started 2026-09-02:** `scripts/safety_comment_scan.py` is the
  mechanized audit (production `unsafe {}` blocks without a `// SAFETY:` in the
  preceding fourteen lines; test modules, `tests/`, `benches/`, `fuzz/` and the
  benchmark crate excluded) and CI runs its `check` mode with a baseline that
  only moves down. The spelling is normalized to `// SAFETY:` (85 `Safety:`
  sites). The largest cluster, `mnemosyne-local/src/free.rs` (18 sites), is
  discharged; baseline **61**, next clusters `local/alloc.rs` (8),
  `decay/lib.rs` (7), `local/realloc.rs` (6), `local_alloc/page/transitions.rs`
  (6), `page/lists.rs` (5).

<a id="mn-436"></a>
- [ ] [major] **MN-436 — preserve allocator mapping provenance.**
  status=review; integrator=codex; branch=`perf/mnemosyne-scratch-release`;
  last-update=2026-09-04. ADR 0009 and merged PRs #75/#79 deliver
  mapping-derived raw pointers, atomic packed heads, `map_addr` tagging, and
  migrated raw segment/page callers. Core, arena, local, and Leto path evidence
  is green; the hosted full-suite Miri run is the final closure gate.

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

