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

<a id="mn-randomized-free-list-guard-regression"></a>
## MN-RANDOMIZED-FREE-LIST-GUARD-REGRESSION — Uncommitted dual-free-list work disables the out-of-bounds abort [major] — review

- **Status:** review; **integrator:** claude-opus-5; **found by:**
  claude-opus-5, 2026-09-07. The re-open trigger fired 2026-09-09: the
  working-tree change is committed and corrected on
  `perf/mnemosyne-scratch-release` (PR #137).
- **Re-measured 2026-09-09, and the original regression is cured.**
  `test_free_list_corruption_out_of_bounds_aborts_process` now **passes**: the
  guard aborts as it should. The working tree has meanwhile grown from 10 dirty
  files to **48**, spanning `local_alloc`, `tls`, `page` types and arena
  scratch, with no commit in 61 hours.
- **A different integrity test is now red.** Full workspace run on that tree:
  **380 passed, 1 failed** — `mnemosyne-local::policy_integration_tests`
  `mixed_encryption_modes_round_trip_without_corruption`. So the work is still
  not committable, for a different reason than when this was filed.
- **Not taken over.** The newest edit was 41 minutes old at the time of
  measurement, inside the stale-claim window, so this is a live peer's work
  and the measurement is recorded rather than the tree claimed. Their files
  were not touched; only this board entry is edited.
- **This item closes when that work commits green**, not when the guard alone
  passes — the original trigger wording ("committed, corrected, or dropped")
  reads too narrowly now that a second failure has appeared under it.
- **Root-caused 2026-09-09, and the cure is a decision rather than a test
  edit.** The tree is now 61 dirty files, 96 minutes past its last edit (so
  reclaimable) and **two** integration tests red:
  `mixed_encryption_modes_round_trip_without_corruption` and
  `test_secure_and_standard_policies_preserve_hardened_segment_encoding`, both
  in `policy_integration_tests`.
  - **Not stranding.** `Page::choose_free_head` (`types/page/mod.rs:228`)
    already forces `randomized` true whenever `secondary_free` is non-empty,
    regardless of the allocating policy's `RANDOMIZE_ALLOCATION`, so no block
    becomes unreachable to a policy that did not free it. The earlier
    entry-condition hole in `try_allocate_page_local` is likewise closed.
  - **What actually fails.** With both lists non-empty, `prefer_secondary_free`
    picks the head, so the allocator legitimately returns the *other* freed
    block. Both tests free under a deliberately mismatched policy and then
    assert **pointer identity** on reuse (`reused_std == ptr_std`) as the proxy
    for "the free-list link remains decodable". Randomization breaks the proxy
    without necessarily breaking the property it stands for.
  - **Why this is not a test to edit.** Those assertions are the executable
    form of [ADR 0001](docs/adr/0001-free-list-encryption-mode-binding.md)
    (Accepted, [arch], "Binding free-list encryption mode to avoid mixed-policy
    corruption"). Work contradicting an Accepted ADR conforms or explicitly
    revises it. So the open question is whether ADR 0001's contract includes
    reuse *identity* or only decodability — and the answer belongs in a dated
    revision of that ADR, alongside a replacement assertion that checks the
    property rather than the proxy (the returned pointer is one of the freed
    blocks and its payload round-trips intact).
  - Not taken further: making that call is the feature author's, and the
    randomization carries no board item or ADR of its own to record it against.
- **What is in the tree.** 251 uncommitted lines across `page/{init,mod,reclaim}.rs`,
  `local_alloc/page/allocation.rs`, `free.rs`, `free_helpers.rs`, `realloc.rs`
  and two test files add a second per-page free list — `Page::secondary_free`
  plus a `RANDOMIZE_ALLOCATION` policy flag — so LIFO reuse does not always
  return blocks in the same order. It carries no board item and no claim.
- **The regression.** `mnemosyne-local tests::test_free_list_corruption_out_of_bounds_aborts_process`
  fails: *"Subprocess succeeded but was expected to abort!"* The test frees a
  block under `StandardPolicy`, writes `0x12345678` as its encrypted next
  pointer, and allocates twice; the second pop must validate the corrupt link
  and `abort()`. With this change in the tree the subprocess exits cleanly —
  the guard no longer fires. `StandardPolicy` has `RANDOMIZE_ALLOCATION`
  false, so this is a regression on the *default* policy path, not only the
  hardened one.
- **Not the test's fault.** That test is untouched by the diff
  (`git diff -- crates/mnemosyne-local/src/tests.rs | grep out_of_bounds`
  is empty). The diff does rewrite the neighbouring
  `hardened_policy_detects_freelist_tamper`, and that rewrite reads as a
  genuine strengthening — it now asserts the decoded next pointer differs
  after tampering rather than asserting on the head address — so it is not
  the cause either.
- **Ruled out so far.** `try_allocate_page_local` is behaviour-neutral under
  `StandardPolicy`: every new disjunct is guarded by `P::RANDOMIZE_ALLOCATION`
  or by `secondary_free.is_none()`, which always holds there. The remaining
  candidates are `Page::try_pop_bump_block`'s new early return, `pop_block`,
  and `reclaim_thread_free_in_segment`'s added parameter — `page/init.rs` is
  the largest hunk at 95 lines and is where the pop-side validation lives.
- **State.** `cargo check --workspace --all-targets` passes; nextest reports
  **356/442 passed, 1 failed, 85 not run** behind the failure. Not committed:
  green per commit is a precondition, and a security guard that stops firing
  is not a state to land and fix later.
- **Also worth deciding before it lands.** `Page` gains an
  `Option<NonNull<Block>>`, and `page_struct_size_stays_within_one_cache_line`
  asserts `size_of::<Page>() <= 64`. That test currently passes, so the field
  fits — but the margin should be recorded, since the whole point of the
  assertion is that page metadata stays one cache line on the allocation hot
  path.


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

## Closed

<a id="mn-pr126-miri-2026-09-04"></a>
- [x] [patch] **MN-PR126-MIRI-2026-09-04 — bound the decay Miri wait.**
  status=done; commit=`9709c57`; refs=PR #129.

<a id="mn-em-pm-compact-1"></a>
- [x] [pm-hygiene][patch] **MNEM-PM-COMPACT-1 — compact backlog and checklist.** status=done; commit=`a3b249a`; refs=backlog.md#mn-em-pm-compact-1.
- [x] [arch] [minor] **MN-469 — Use Melinoe permits for branded heap handoff.**; refs=afbedd4
- [x] [patch] **MN-THEMIS-AFFINITY-CONSUMER-2026-09-01.** status=complete;; refs=PR #87,49146cdd98fa0457082f7f3da7ca9df9ea30f7a7
- [x] [patch] **MN-468 — the benchmark harness parses its own performance-core; refs=PR #86,dde4012
- [x] [patch] [perf] **MN-464 — the threshold baseline is noisier than the; refs=0cfd33e
- [x] [patch] [perf] **MN-466 — `huge_shrink_4m_to_2m` does not reproduce; refs=git-history
- [x] [patch] [perf] **MN-467 — capture the threshold baseline under the pinned; refs=185f828
- [x] [patch] **MN-462 — the SnMalloc comparator column cannot be produced on any MSYS2-flavoured Windows host.** status=done (2026-09-04); integrator=codex; branch=`perf/mnemosyne-scratch-release`; lease=discharged; PR=#128.; refs=PR=#128
- [x] [patch] **MN-463 — the Windows Jemalloc column needs an MSVC-built; refs=git-history
- [x] [patch] **MN-465 — `mnemosyne-backend` does not lint clean for; refs=git-history
- [x] [patch] **MN-CONFORMANCE-COMMENTED-CODE-2026-08-31 — restore the; refs=PR #82,67a8b54
- [x] [minor] **MN-458 — close the retag, provenance, and cold-branch; refs=PR #79,4682a79
- [x] [patch] **MN-460 — add a semver gate for the publishable crates.**; refs=0253866f8
- [x] [minor] [perf] **MN-461 — 16 KiB is the only size where this allocator loses to the system allocator.**; refs=03d80d33
- [x] [patch] **MN-457 — scope the Windows CUDA thread-exit import to; refs=8be9ec3
- [x] **MNEM-DIAG-1** [patch] status=done owner=atlas-session; refs=git-history
- [x] **MNEM-CI-BENCH-1** [patch] status=done owner=atlas-session; refs=git-history
- [x] **MNEM-SUPPLY-1** [security][patch] status=done owner=Claude; refs=PR #73,80b1a2a
- [x] **MNEM-SEMVER-1** [patch] status=done owner=Claude; refs=b75fe12,7ffd08e
- [x] **MNEM-FUZZ-CI-1** [patch] status=done 2026-09-02 owner=Claude — delivered; refs=33600789737,fdf66542e
- [x] **MNEM-RSS-1** [verification][patch] status=complete; commit=`22361e9`; lease=none.; refs=22361e9
- [x] **MNEM-THP-TEST-1** [verification][patch] status=review; commit=`e694c1e`;; refs=PR #83,PR #84
- [x] **MNEM-DOCS-GAP-1** [docs][patch] status=done owner=Claude; refs=git-history
- [x] **MNEM-ADR-INDEX-1** [docs][patch] status=done owner=claude (2026-09-01); refs=git-history
- [x] **MNEM-MISSINGDOCS-1** [arch][patch] status=done owner=Claude; refs=git-history
- [x] **MNEM-LINTFLOOR-1** [pm-hygiene][patch] status=done owner=claude (2026-09-01); refs=git-history
- [x] [patch] **ATLAS-MNEMOSYNE-CONFORMANCE-101 — close the fifth; refs=39d76d2,32024295467
- [x] [patch] **Internal policy SSOT consolidation — implemented; validation; refs=git-history
- [x] [patch] Publish future releases through a pinned GitHub Actions workflow; refs=git-history
- [x] [patch] Exclude cyclic workspace-only dev-dependencies from published; refs=git-history
- [x] [patch] **MNEM-THEMIS-PACKAGE-1 — restore Themis resolution.** Owner:; refs=git-history
- [x] [major] Publish the allocator facade and core under the collision-free; refs=git-history
- [x] [major] **WGPU-030, done; owner Codex; scope; refs=git-history
- [x] [major] Follow Eunomia and Melinoe provider default branches from the; refs=git-history
- [x] [patch] status=done owner=codex scope=`mnemosyne-arena`; fix concurrent; refs=PR #9,01e7de7
- [x] [patch] status=done owner=codex scope=`Cargo.toml`, `Cargo.lock`, and; refs=git-history
- [x] [arch] Stage D1: device-memory strategy consumed by the `hephaestus` GPU substrate; refs=git-history
- [x] [minor] status=done owner=codex scope=`crates/mnemosyne-backend/src/backends/cuda/mod.rs`, `crates/mnemosyne-arena/src/segment/pool/mod.rs`, `crates/mnemosyne-local/src/lib.rs`, `crates/mnemosyne-heap/src/{tiered_back...; refs=git-history
- [x] [minor] `KernelResourceBudget` (`mnemosyne-core::kernel_budget`):; refs=git-history
- [x] [patch] Stack-interner final-release critical section. The final entry; refs=git-history
- [x] [patch] `AlignedVec::into_vec` source-buffer release. Conversion keeps; refs=git-history
- [x] [patch] Page-metadata provenance and remote-free aliasing. Cached page; refs=git-history
- [x] [patch] Refresh `mnemosyne-local` to Melinoe 0.9.0 so the allocator and; refs=git-history
- [x] [patch] Atlas provider graph refresh. `mnemosyne-local` now requires; refs=git-history
- [x] [patch] Eunomia scratch local-source contract. `mnemosyne` and; refs=git-history
- [x] [major] AR-2 WGPU callback registration soundness (superseded by ADR 0003). The public; refs=git-history
- [x] [major] WGPU callback registration publishes one immutable; refs=git-history
- [x] [minor] Replace the always-resident 720,896-byte per-CPU cache table with; refs=git-history
- [x] [patch] **MN-437 — Miri-reported UB in the reclamation seam.** Fixed.; refs=git-history
- [x] [arch] **MN-438 — segment-addressed page access.** Complete. The; refs=git-history
- [x] [arch] **MN-439 — shared segment metadata is non-atomic.** Done for the; refs=git-history
- [x] [arch] **MN-440 — the re-entrancy guard lives inside the object it; refs=git-history
- [x] [major] **MN-442 — the huge-allocation classifier read uninitialized; refs=git-history
- [x] [major] **MN-443 — forbidden wildcard write in page occupancy under Tree; refs=git-history
- [x] [patch] **MN-445 — mnemosyne-local joins the Miri job.** Done. Both; refs=git-history
- [x] [patch] **MN-446 — decay purger lifecycle synchronization.** Closed; refs=PR #54,dce730789a22adc4e39c513015c0a3b36ad1934b
- [x] [patch] **MN-447 — no test drove page recycling.** Done.; refs=git-history
- [x] [major] **MN-448 — allocator stats ignored the policy and reported the; refs=git-history
- [x] [major] **MN-451 — `ensure_options_initialized` has one export path.**; refs=git-history
- [x] [patch] **MN-452 — the saturation test's slow margin.** Done, and the; refs=git-history
- [x] [patch] **MN-450 — the test-only export is out of the entry-point; refs=git-history
- [x] [patch] **MN-449 — `mnemosyne-local` ships doctests.** Done for the; refs=git-history
- [x] [patch] **MN-444 — Miri leak checking is on for mnemosyne-local.** Done,; refs=git-history
- [x] [patch] **MN-441 — `is_current` is owner-only by construction.** Done, by the second of the two; refs=e421584
- [x] [patch] **MN-454 — `mnemosyne-backend` compiles and passes under Miri.**; refs=git-history
- [x] [major] **MN-455 — the pool stack's node link is atomic.** Done.; refs=git-history
- [x] [patch] **MN-456 — preserve `RawHeap` mapping provenance.** Done by; refs=6c778b5
- [x] [arch] **MN-433 — concurrency model checking.** Done; the seam and all; refs=git-history
- [x] [patch] **MN-434 — the SeqCst audit.** Done, and the premise was wrong:; refs=git-history
- [x] [patch] **MN-453 — TSan covers the global-allocator path.** Done, and the; refs=PR #63,32198740189
- [x] [patch] **MN-435 — aarch64 and ThreadSanitizer jobs.** Done; both run in; refs=32183974171
- [x] [patch] status=done owner=codex scope=`Cargo.toml`, the affected package; refs=6a4bad7,1070417
- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-benchmarks/benches/allocator/workers.rs` and `crates/mnemosyne-local/src/local_alloc/tests/`; last-update=2026-07-25. Restored workspace rustfmt cleanliness with...; refs=git-history
- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-heap/src/heap.rs`, `crates/mnemosyne-heap/src/tests/`, and matching PM entries; last-update=2026-07-24. Compute the branded block's runtime layout before `drop_i...; refs=git-history
- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-heap/src/branded_vec.rs`, `crates/mnemosyne-heap/src/tests/`, and matching PM entries; last-update=2026-07-24. Removed `unreachable_unchecked` from the fallible ...; refs=git-history
- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-heap/src/{heap.rs,tiered_heap.rs}`, `crates/mnemosyne-heap/src/tests/`, and matching PM entries; last-update=2026-07-24. Validate the caller-supplied source `Lay...; refs=git-history
- [x] [major] status=done owner=codex scope=`crates/mnemosyne-heap/src/{heap.rs,raw_heap.rs,branded_vec.rs,tiered_heap.rs}`, `crates/mnemosyne-heap/src/tests/`, `crates/mnemosyne/src/lib.rs`, and matching PM entries; last-u...; refs=git-history
- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-prof/src/{lib.rs,sampler/{mod.rs,store.rs},tests.rs}`, `crates/mnemosyne-benchmarks/benches/allocator/{profiler,mod}.rs`, `allocator_bench.rs`, and profiler PM e...; refs=git-history
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-prof/src/sampler/`,; refs=PR #17,1c91baf
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-prof/src/sampler/capture.rs`; refs=PR #18,3a6b643
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-prof/src/sampler/store.rs`; refs=PR #19,a281082
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-prof/src/sampler/sampling.rs`; refs=PR #20,7046976
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-prof/src/sampler/report.rs`; refs=git-history
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-arena/src/segment/{mod.rs,alloc.rs,alignment.rs}` and `crates/mnemosyne-benchmarks/benches/allocator/{mod.rs,allocation.rs,failure.rs,platform.rs,registration.rs,...; refs=git-history
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-local/src/local_alloc/page.rs`, new `page/` leaves, callers, and matching PM entries; last-update=2026-07-26. Split the 546-line page module into named leaves for...; refs=git-history
- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-core/src/{size_class.rs,types/page/init.rs}` and `crates/mnemosyne-local/src/local_alloc/routing.rs`, with matching PM entries; last-update=2026-07-26. Replaced ...; refs=git-history
- [x] [patch] **Shared helper for the cached-pointer TLS fast path — decided; refs=git-history
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-core/src/{sync.rs,types/{block.rs,page/reclaim.rs,segment.rs}}`, `crates/mnemosyne-local/src/{alloc.rs,free.rs,per_cpu.rs,realloc.rs,local_alloc/,lib.rs,tls_slot....; refs=git-history
- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-benchmarks/benches/segment_lock.rs`, `crates/mnemosyne-benchmarks/Cargo.toml`, and contention PM entries; last-update=2026-07-15. Isolate `CacheAlignedSegmentLoc...; refs=git-history
- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-arena/src/segment/pool/{tagged_stack,cache_aligned}.rs`, allocator benchmarks, and contention PM entries; last-update=2026-07-15. Measure the per-stack lifetime-...; refs=PR #9,2adec54
- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-local`,; refs=git-history
- [x] [patch] Complete the Miri page-metadata provenance fix before resuming the; refs=5a9f49f
