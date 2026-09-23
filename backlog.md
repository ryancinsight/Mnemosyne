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
    main by other routes; what it still held alone was **purge-and-retry on
    the first OS allocation failure**. **Landed 2026-09-23**, re-derived
    natively against current main in `crates/mnemosyne-arena/src/segment/alloc/allocate.rs`
    rather than ported from the stale branch: bounded to one purge and one
    retry, with the attempt and its outcome surfaced through two new
    counters (`ArenaMemoryStats::oom_retries` / `oom_retry_successes`,
    matching `SegmentPoolStats`) and a test
    (`segment::tests::first_os_allocation_failure_purges_and_retries`) that
    forces the first `B::allocate` call to fail and asserts the retry
    succeeds and both counters move. This branch's content is now fully
    accounted for.
  - `origin/feat/phase10-improvements` (`a07f999d`) -- 25 commits ahead of
    main. **Assessed 2026-09-23: superseded.** Of the 60 public items its
    diff adds, 58 resolve on main by name; the other two have main
    equivalents (`AlignedVecIntoIter` -> main's owned `IntoIterator` for
    `AlignedVec`, `resize_fill` -> `resize` then `fill`).
- **Integrator: claude-opus-5.5 (takeover 2026-09-23).** The item carried no
  integrator and no recorded activity; taken over to land the
  free-helpers-split branch's sole remaining content.
- **Why filed rather than ported.** Both predate the free-list randomization
  that landed in #137 and touch the same allocator paths, so a rebase is a
  re-derivation, not a merge. Each branch's unique commits are read and
  either re-derived against current main or dropped with the reason recorded.
- **Acceptance:** every commit on both branches is either re-derived onto
  main or recorded here as superseded, and the branches are deleted from
  origin. Both are now accounted for; **remaining:** delete
  `refactor/mnemosyne-free-helpers-split` (`e26ee023`) and
  `feat/phase10-improvements` (`a07f999d`) from origin. Blocked on
  permission: the session's remote-branch deletion was refused and needs
  the owner.

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
- **Rejected 2026-09-23:** an exact-growth variant for the bounded path
  (`ensure_len_exact`, from a stranded local series) contradicts this
  acceptance -- it drops amortized doubling -- and main already trims to the
  provision in `release`. Remaining: the growth-events regression test.
- **Risk / delivery:** `[patch]` private growth policy and regression coverage;
  integrator current Atlas session; branch `perf/scratch-release`.

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

