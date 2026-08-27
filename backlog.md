# Backlog

## Ready — gap-audit 2026-08-20 intake

Filed by the Atlas gap audit at detached `HEAD` `6b0e490`. Evidence for every
item is in `gap_audit.md` under "Finding 2026-08-20: mnemosyne
scope-vs-delivery audit". These are DoR-shaped and unclaimed; the audit ran
static-only (no build/test/clippy), so each item's first step is to reproduce
its cited grep against current `HEAD` before editing (stale-memory rule).

- [x] **MNEM-DIAG-1** [patch] status=done owner=atlas-session
  scope=`crates/mnemosyne-core/src/memory_diagnostics.rs`,
  `crates/mnemosyne/src/lib.rs`, and whichever alloc/free path is chosen as
  the recording site. Non-goals: adding hot-path atomics to the small-alloc
  fast path; changing `MemoryStats`/`BackendMemoryStats`/`ArenaMemoryStats`.
  **Outcome:** `AllocationDiagnostics` either records real allocator activity
  or stops being public. Today `record_allocation`, `record_cache_hit`, and
  `record_cache_miss` have zero call sites workspace-wide, and
  `fragmented_blocks` / `page_utilization_percent` have zero writers, yet
  `MemoryEfficiencyReport::from_diagnostics` computes a
  `fragmentation_overhead` from them and the type is re-exported from the
  facade (`crates/mnemosyne/src/lib.rs:17-18`). **Acceptance oracle:** either
  (a) a value-semantic test drives N allocations and M frees across ≥2 size
  classes through the public allocator and asserts the exact resulting
  `total_allocations`, `live_allocations`, per-class `allocation_count`, and a
  non-trivial `fragmentation_overhead`; or (b) the module and its three
  facade re-exports are deleted and `cargo package --dry-run` on
  `mnemosyne-memory-core` and `mnemosyne-memory` succeeds. Decide by whether a
  cold-path recording site exists that does not cost the fast path — if it
  does not, (b) is the correct answer. **Dependencies:** none.
  **Risk/change class:** [patch] if deleted from a pre-1.0 surface; run
  `cargo-semver-checks` either way. **Effort:** S (delete) / M (wire).

- [x] **MNEM-CI-BENCH-1** [patch] status=done owner=atlas-session
  scope=`.github/workflows/ci.yml`, `crates/mnemosyne-benchmarks/Cargo.toml`.
  Non-goals: changing any benchmark's measured scenario, inputs, or timed
  region; changing baselines. **Outcome:** delivered in this PR — the
  SnMalloc comparator that could not build on hosted runners is opt-in via
  the `snmalloc` feature (jemalloc stays default on non-Windows), all four
  `--exclude mnemosyne-benchmarks` flags are dropped, and a
  `benchmark-hygiene` job runs workspace-floor clippy on the crate plus a
  single-pass Criterion smoke (`cargo test --benches`, test mode). Lint debt
  exposed by first-time coverage fixed at its sites; segment_lock's
  source-include carries a scoped `#[expect(dead_code)]` for the arena-only
  `try_lock`. **Acceptance oracle:** a CI job
  runs `cargo clippy -p mnemosyne-benchmarks --all-targets -- -D warnings` and
  a single-iteration Criterion smoke (`cargo test --benches -p
  mnemosyne-benchmarks`, i.e. Criterion's `--test` mode) inside the job's
  finite budget, both green — satisfied by the new job (12-minute
  `timeout-minutes`, sized to the five-minute target plus the Linux jemalloc
  source build). If a comparator dependency (`snmalloc-rs`,
  jemalloc) cannot build on a runner, gate that comparator behind a feature
  rather than excluding the whole crate — done for snmalloc.
  **Dependencies:** none.
  **Risk/change class:** [patch]. **Effort:** M.

- [ ] **MNEM-SUPPLY-1** [security][patch] status=todo owner=unclaimed
  scope=`.github/workflows/ci.yml`, new `deny.toml`. Non-goals: changing any
  dependency version; adopting Dependabot/Renovate (separate item).
  **Outcome:** the workspace has advisory, license, ban, duplicate,
  unused-dependency, and yanked-crate enforcement. Today `grep -rli` over
  `.github/workflows` finds no `cargo-deny`, `cargo-audit`,
  `cargo-machete`, `cargo-geiger`, or `cargo-semver-checks` step, and no
  `deny.toml`/`audit.toml` exists — for eleven crates.io-published package
  identities. **Acceptance oracle:** a `supply-chain` job runs `cargo deny
  check advisories bans licenses sources` and `cargo machete` green against a
  committed `deny.toml`, and `cargo semver-checks check-release` runs on any
  PR touching a `pub` item (see MNEM-SEMVER-1). Triage exposure from `cargo
  tree -e normal`, never lockfile presence. **Dependencies:** none.
  **Risk/change class:** [patch]. **Effort:** M.

- [ ] **MNEM-SEMVER-1** [patch] status=todo owner=unclaimed
  scope=`.github/workflows/ci.yml`. Non-goals: changing any public API.
  **Outcome:** `cargo-semver-checks` becomes a standing gate rather than a
  manually-run step recorded in closed backlog entries. The audit found no
  semver job in any workflow, while `CHANGELOG.md` Unreleased already carries
  two entries marked **Breaking** (`Segment::next_free_segment` becoming
  `AtomicPtr`, and `ensure_options_initialized` leaving the crate root).
  **Acceptance oracle:** the job runs on every PR touching a published crate's
  `pub` surface, and its classification is authoritative over the PR's
  declared change class — a detected break under a `[patch]`/`[minor]` label
  fails the gate. Verify by confirming it flags the two Unreleased breaks.
  **Dependencies:** MNEM-SUPPLY-1 may host the same job. **Risk/change
  class:** [patch]. **Effort:** S.

- [ ] **MNEM-FUZZ-CI-1** [patch] status=todo owner=unclaimed
  scope=`.github/workflows/ci.yml`, `fuzz/`. Non-goals: writing new fuzz
  targets (separate item if the arena/segment surface warrants one).
  **Outcome:** the committed `c_shim_api` libFuzzer target actually runs. It
  covers the crate's one untrusted-input boundary (the C ABI: `malloc`,
  `free`, `calloc`, `realloc`, `aligned_alloc`, `posix_memalign`,
  `malloc_usable_size`) and today no workflow references `fuzz` at all.
  **Acceptance oracle:** a scheduled or merge-triggered job runs `cargo
  +nightly fuzz run c_shim_api` for a committed finite duration under the
  nightly verification toolchain, green, with any discovered crash committed
  to the corpus as a regression. State the time budget explicitly.
  **Dependencies:** none. **Risk/change class:** [patch]. **Effort:** S.

- [ ] **MNEM-RSS-1** [verification][patch] status=todo owner=unclaimed
  scope=`crates/mnemosyne-decay/tests/`, `crates/mnemosyne-arena/tests/`.
  Non-goals: shrinking any existing test's workload; adding hot-path counters.
  **Outcome:** a bounded-memory argument exists for a long-running arena.
  Today `decay_purger_reaches_steady_state`
  (`mnemosyne-decay/tests/decay_tests.rs:142`) asserts retained-*segment
  count* convergence only; `grep -rni 'live_bytes'` returns 0; no test bounds
  fragmentation over an adversarial mix. **Acceptance oracle:** a test runs an
  alternating-size-class workload with pinned survivors for a bounded number
  of rounds and asserts that `arena_memory_stats().current_mapped_bytes` (or
  the equivalent retained-bytes accessor) converges within a derived bound of
  live bytes — the bound stated with its derivation, not a tuned epsilon —
  and completes inside the 30 s nextest budget. If the workload cannot fit
  that budget, that is a performance defect to profile, not a reason to move
  it out of the suite. **Dependencies:** MNEM-DIAG-1 if the oracle uses
  `fragmentation_overhead`. **Risk/change class:** [patch]. **Effort:** M.

- [ ] **MNEM-THP-TEST-1** [verification][patch] status=todo owner=unclaimed
  scope=`crates/mnemosyne-backend/src/backends/unix.rs`,
  `crates/mnemosyne-backend/src/recorders.rs`. Non-goals: changing when the
  hint is issued. **Outcome:** the three huge-page-hint tests assert the
  behavior their names claim. `sub_segment_allocation_skips_hugepage_hint`
  (`unix.rs:324`) and `large_non_multiple_allocation_receives_hugepage_hint`
  (`unix.rs:344`) each assert only allocate/write/deallocate round-trip;
  replacing `hint_hugepage`'s body with a no-op leaves all three green.
  **Acceptance oracle:** `hint_hugepage` records its decision through the
  existing `recorders` telemetry (a `hugepage_hint_calls` counter, `#[cfg]`-
  scoped exactly like the existing `page_reset_calls`), and each of the three
  tests asserts the exact counter delta — 0 for the sub-segment case, 1 for
  both ≥ `SEGMENT_SIZE` cases. A no-op `hint_hugepage` must then fail two of
  the three. **Dependencies:** none. **Risk/change class:** [patch].
  **Effort:** S.

- [ ] **MNEM-DOCS-GAP-1** [docs][patch] status=todo owner=unclaimed
  scope=`docs/gap_analysis_external.md`, `docs/complexity_audit.md`. Non-goals:
  re-surveying the allocator literature; changing any priority tag whose
  underlying assessment still holds. **Outcome:** the external gap analysis
  stops asserting the tree's state incorrectly. Six verified drifts: §2 names
  a `local_free` page field that does not exist; §3 states
  `MAX_RETAINED_SEGMENTS = 32` when `MAX_RETAINED_SEGMENTS_LIMIT = 1024`
  (`mnemosyne-core/src/constants.rs:40`); §4 calls huge-mapping retention "Not
  implemented" although `huge_pool.rs` retains per NUMA bucket under a byte
  budget; §5 calls NUMA-aware arena selection "Not implemented" although
  `numa_bucket.rs` + `segment_pool.rs:112-162` implement it; §7 calls
  per-allocation profiling and alloc/free hooks "Not implemented" although
  `mnemosyne-prof` ships both; §9 calls `posix_memalign`/`aligned_alloc`
  indirect although `mnemosyne-c-shim/src/lib.rs:187,216` export them. §11.2
  speaks of `page_reset`/`make_guard` in the future conditional; §12 names two
  test guards (`c_shim_round_trip_matches_global_alloc`,
  `runtime_options_override_default_retention`) that return 0 grep hits; §12
  repeats the wrong `size % SEGMENT_SIZE == 0` huge-page condition already
  corrected in `README.md`. `docs/complexity_audit.md` says "11-crate
  workspace" for 12 members and its map omits four crates. **Acceptance
  oracle:** every row's state column is re-derived from a cited
  `path:line`, and a grep for each named test guard resolves.
  **Dependencies:** none. **Risk/change class:** [patch]. **Effort:** M.

- [ ] **MNEM-ADR-INDEX-1** [docs][patch] status=review owner=codex
  scope=`docs/adr/README.md`, new `scripts/` or `xtask`. Non-goals: rewriting
  ADR content. **Outcome:** the ADR index's generator claim becomes true.
  `docs/adr/README.md` reads "Generated by scripts/adr-index.py — do not
  hand-edit. Regenerate: python scripts/adr-index.py generate"; there is no
  `scripts/` directory and `find . -name 'adr-index.py'` returns nothing, so
  the index has no freshness check and the instruction is unrunnable.
  **Acceptance oracle:** either the generator is committed and a CI step runs
  its `check` mode (regenerate-and-diff, failing on drift), or the header is
  replaced with the truth that the index is hand-maintained. Also normalize
  ADR 0001's status line from "Accepted and implemented" to the canonical
  `Accepted`. **Dependencies:** none. **Risk/change class:** [patch].
  **Effort:** S.

- [ ] **MNEM-MISSINGDOCS-1** [arch][patch] status=todo owner=unclaimed
  scope=`crates/mnemosyne-core/src/lib.rs`,
  `crates/mnemosyne-arena/src/lib.rs` and whatever public items the deny then
  flags. Non-goals: the other ten crates (already conforming);
  `mnemosyne-benchmarks` (`publish = false`). **Outcome:** every published
  crate denies undocumented public API. These two are the only published
  crates without `#![deny(missing_docs)]`, and they hold the core layout types
  (`Block`/`Page`/`Segment`), size classes, validation predicates, the policy
  ZSTs, and the whole segment/arena/scratch surface. **Acceptance oracle:**
  both `lib.rs` files carry `#![deny(missing_docs)]` and `cargo doc --no-deps`
  is warning-clean for both packages under `RUSTDOCFLAGS=-D warnings`. Each
  doc added must state the item's contract, not restate its signature.
  **Dependencies:** none. **Risk/change class:** [patch]. **Effort:** M —
  size it by first running the deny locally and counting the flagged items.

- [ ] **MNEM-UNSAFE-DOC-1** [verification][patch] status=todo owner=unclaimed
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

- [ ] **MNEM-PM-COMPACT-1** [pm-hygiene][patch] status=todo owner=unclaimed
  scope=`backlog.md`, `checklist.md`. Non-goals: deleting evidence — closed
  items keep their commit/PR references; touching any in-progress claim.
  **Outcome:** a cold-start agent can find the ready work in one place.
  `backlog.md` is 1,633 lines with 65 completed items retained as full prose,
  5 unchecked, and four competing section headings (`## Open` at 206 and 1571,
  `## Closed` at 140 and 189, `## Completed` at 996, `## Next` at 1628);
  `checklist.md` is 1,529 lines with the same shape. **Acceptance oracle:**
  one `## Ready`, one `## In progress`, one `## Blocked`, and one `## Closed`
  section; every closed item collapsed to a one-line entry with its commit/PR
  link; every remaining open item carrying the board schema (stable ID,
  outcome, scope/non-goals, acceptance oracle, dependencies, risk/change
  class, status, owner, last-update). Compaction precedes the next
  replenishment. **Dependencies:** none — but do this before filing further
  items. **Risk/change class:** [patch]. **Effort:** M.

- [ ] **MNEM-LINTFLOOR-1** [pm-hygiene][patch] status=todo owner=unclaimed
  scope=`Cargo.toml` `[workspace.lints]` block,
  `crates/mnemosyne-core/src/kernel_budget.rs`,
  `crates/mnemosyne-local/src/tests.rs`. Non-goals: loosening any lint level;
  changing the pedantic ratchet baselines. **Outcome:** the lint-floor comment
  matches the tree. It states the 24 `unwrap` sites "are pinned per file by
  `#![expect(..., reason = "MNEM-UNWRAP-1")]`, which self-expires the moment a
  file's last unwrap goes". There are 0 `expect` attributes in the workspace;
  both sites use `#![cfg_attr(test, allow(clippy::unwrap_used, reason =
  "test scope: ..."))]`, which does not self-expire, and `MNEM-UNWRAP-1`
  appears nowhere else. **Acceptance oracle:** either both sites migrate to
  `expect` (verifying the attribute survives the `cfg_attr(test, ...)` wrapper
  under the pinned toolchain) and the comment's ratchet claim becomes true, or
  the comment is corrected to describe `allow`. Add `clippy::allow_attributes`
  to the floor so the migration is mechanized thereafter. **Dependencies:**
  none. **Risk/change class:** [patch]. **Effort:** S.

- [ ] **MNEM-BOOK-DEPTH-1** [docs][minor] status=todo owner=unclaimed
  scope=`docs/book/`. Non-goals: the two factual rewrites already landed
  (`size_classes.md`, `numa_placement.md`); adding chapters with no teaching
  content. **Outcome:** the book teaches allocator design from the ground up,
  as the domain-book contract requires, rather than summarizing the README.
  All ten numbered chapters are 36–54 lines, and only two of them
  (`alloc_policies`, `scratch_pools`) have an executable example under
  `mdbook test`. The theory layer a newcomer needs — fragmentation as a
  metric and why size classes bound it, the happens-before argument behind the
  page-local cross-thread queue, decay/purge as an RSS-vs-syscall tradeoff,
  why free-list encryption detects the UAF classes it does — is absent, and
  the sources are already cited in the README's Research Foundations table.
  **Acceptance oracle:** each Part has at least one chapter carrying a
  derivation or protocol argument with a resolved citation (paper + section),
  and each Part has at least one `mdbook test`-executed example. File one
  DoR-shaped sub-item per chapter rather than treating this as one edit; this
  entry is the parent. **Dependencies:** MNEM-DOCS-GAP-1 (the gap analysis is
  the source for several chapters' "what we do not do" sections).
  **Risk/change class:** [minor]. **Effort:** L.


## ATLAS-MNEMOSYNE-BOOK-TEST-2026-08-20 — Execute included book examples [patch] — done 2026-08-20

- **Scope:** shared Pages caller and the two included allocator examples;
  allocator implementation and the peer-staged `Cargo.lock` remain outside
  this increment.
- **Acceptance:** the caller pins the shared Atlas workflow, enables
  `mdbook-test`, stages package `mnemosyne-memory` under library target
  `mnemosyne`, and hosted CI executes both included examples with Rust 1.97.0.
- **Baseline:** local `mdbook test docs/book` reached both examples but failed
  with unresolved `mnemosyne` imports because the caller provided no staged
  library or explicit crate declaration.
- **Landed source:** source `a527380a15e8979c3b773a4e9891f1d53b0bc45c`, PR #65,
  and merged default `7003eb3d09a716a91b4560e1810d65970c874daa`. Exact PR Rust,
  MSRV, Loom, TSan, aarch64, Miri, and Pages checks pass; post-merge CI
  `32341399588`, MSRV `32341399599`, and Deploy mdBook `32341400004` pass.
  Live Pages returns HTTP 200 with the expected Mnemosyne title.
- **Delivery:** the Atlas gitlink records the merged default and this provider
  item is closed.

- [x] [patch] **ATLAS-MNEMOSYNE-CONFORMANCE-101 — close the fifth
  existence-only assertion (2026-08-17).** Replaced the NUMA binding test's
  `is_ok()` assertion with an exact `Ok(())` value assertion in
  `crates/mnemosyne-heap/src/tests/numa.rs`. The clean provider branch at
  `39d76d2` passed hosted Rust verification, Loom, and Miri (both Stacked and
  Tree Borrows) in run `32024295467`; the report-only `recurseml/analysis`
  status was external and did not block merge. The conformance scan baseline
  is now 4 existence-only assertions.

- [x] [patch] **Internal policy SSOT consolidation — implemented; validation
  blocked (2026-08-06).** `mnemosyne-core::policy` is the sole implementation
  and canonical internal import surface for `SecurePolicy` and `HardenedPolicy`.
  Removed the redundant internal `mnemosyne-hardened` dependency from the
  `mnemosyne-memory`, `mnemosyne-local`, and `mnemosyne-benchmarks` manifests;
  migrated the facade and all local policy tests to `mnemosyne-core`; and removed
  exactly those three stale edges from `Cargo.lock`. The
  `mnemosyne-hardened` package remains a deliberately thin compatibility
  re-export for external dependents, so the public historical package identity
  is preserved without a second implementation. Source audit finds no internal
  `mnemosyne_hardened` imports outside that compatibility crate; its deliberate
  `mnemosyne-hardened` workspace/lock entry remains only for that compatibility
  package. Rustfmt, `git diff --check`, and per-package
  `cargo metadata --manifest-path ... --locked --offline --no-deps` resolve.
  Full root-workspace check/Clippy/Nextest/doctest gates remain blocked by the
  pre-existing workspace overlay:
  Cargo `--locked` attempts to reconcile stale patch/source metadata, while the
  unlocked diagnostic run reached unrelated missing-docs failures and then ran
  out of disk space. No unrelated registry lock refresh was retained.

- [x] [patch] Publish future releases through a pinned GitHub Actions workflow
  using crates.io OIDC Trusted Publishing and no stored registry credential.

- [x] [patch] Exclude cyclic workspace-only dev-dependencies from published
  manifests by leaving them path-only, as defined by Cargo's publication
  contract. Runtime and build dependencies retain registry versions.

- [x] [patch] **MNEM-THEMIS-PACKAGE-1 — restore Themis resolution.** Owner:
  Codex on `codex/mnemosyne-themis-package`. Bind the existing Rust crate alias
  to upstream package `themis-topology` 0.10.1; refresh the lockfile; pass
  focused checks; merge before dependent Hephaestus provider CI is retried.

- [x] [major] Publish the allocator facade and core under the collision-free
  `mnemosyne-memory` and `mnemosyne-memory-core` package identities. Retain
  the existing Rust crate names through explicit library targets and dependency
  aliases. Decision: [ADR 0007](docs/adr/0007-crates-io-package-identities.md).

- [x] [major] **WGPU-030, done; owner Codex; scope
  `mnemosyne-backend`, facade re-exports, backend selector impls/tests/docs, and
  release artifacts; last update 2026-07-13.** Remove the process-global WGPU
  raw-pointer staging backend. WGPU 30 exposes mutable mapped ranges only
  through a write-only view, so the `MemoryBackend` pointer contract cannot be
  implemented without violating the provider's memory model. Acceptance:
  obsolete callbacks and selectors are deleted and the remaining backends pass
  the workspace gates. Hephaestus migration is tracked by Atlas WGPU-030.

- [x] [major] Follow Eunomia and Melinoe provider default branches from the
  workspace SSOT, raise the published Rust MSRV to 1.95, and advance the
  pre-1.0 package versions. `Cargo.lock` remains the reproducibility pin.

- [x] [patch] status=done owner=codex scope=`mnemosyne-arena`; fix concurrent
  segment-head reclamation exposed by the RITK registration suite. The fix is
  merged in PR #9 (`01e7de7`, implementation `09b2ef8`); provider gates and the
  consumer wheel boundary are green.

- [x] [patch] status=done owner=codex scope=`Cargo.toml`, `Cargo.lock`, and
  release artifacts; remove the Themis revision quarantine so consumers follow
  its default branch through one canonical source identity. Acceptance: focused
  allocator gates are green and the Moirai consumer no longer duplicates the
  Themis source. The current manifest uses the canonical git-plus-version
  dependency without a `rev` pin or path override; the checked item is closed
  and has no remaining provider-side action.

## Atlas in-house replacement roadmap — mnemosyne slice [arch]

mnemosyne is the allocation SSOT. The GPU program (coeus/apollo using wgpu + cuda-oxide)
needs a first-class device-memory story beyond the current dlopen `CudaUnifiedBackend`:
- [x] [arch] Stage D1: device-memory strategy consumed by the `hephaestus` GPU substrate
  (atlas ADR 0001) — device buffer pools, page-locked/pinned host staging, explicit
  unified-vs-discrete policy through the `MemoryBackend` seam. Compose cuda-oxide
  allocation interop with the existing dlopen `cuMemAllocManaged` path; add wgpu
  buffer-pool hooks. ADR.
- [ ] [minor] status=blocked owner=external integration scope=`mnemosyne-heap`,
  `hephaestus-core`, and the paired coeus Stage D consumer; last-update=2026-07-24.
  Stage D1: melinoe-branded device buffers so ownership transfer between
  host/device/stream is a compile-time proof. Blocker: Mnemosyne's
  `MemoryBackend` owns raw mapped allocation/free, Melinoe supplies only
  lifetime-branded tokens, and the current provider-owned Hephaestus
  `DeviceBuffer<T>` plus stream/synchronize contracts carry device allocation
  and completion semantics. A second Mnemosyne wrapper would duplicate the
  provider seam and cannot prove asynchronous stream lifetime. Re-open when
  the provider contract exposes a branded device/stream ownership boundary and
  a concrete consumer integration target; do not add a downstream adapter.

### Heterogeneous tiers + kernel resource budgets (atlas ADR 0002)
- [x] [minor] status=done owner=codex scope=`crates/mnemosyne-backend/src/backends/cuda/mod.rs`, `crates/mnemosyne-arena/src/segment/pool/mod.rs`, `crates/mnemosyne-local/src/lib.rs`, `crates/mnemosyne-heap/src/{tiered_backend.rs,tiered_heap.rs,tier.rs}`, `crates/mnemosyne-decay/src/lib.rs`, matching tests/docs, and package metadata; Tier-keyed device pools: allocation keyed by themis
  `MemoryTier` (`Hbm` vs the new `Gddr`) + `PlacementHint`, with pinned-host
  (`HostPinned`) staging pools, behind the existing `MemoryBackend` seam. The
  new HBM/GDDR backends are zero-sized static-dispatch identities over the
  shared CUDA managed-memory driver, with independent segment pools, TLS
  selectors, and decay coverage; no physical technology-placement guarantee
  is claimed because the current driver API does not expose one. Evidence:
  full debug workspace nextest 281/281; affected debug slice 129/129;
  warning-denied workspace Clippy; workspace doctests/rustdoc; semver checks
  for backend 0.5.0, heap 0.3.0, and facade 0.6.0. Release workspace slice
  is 199/200 because the existing stripped-release leak-detector symbol
  assertion fails; no allocator test was weakened. Last-update=2026-07-23.
- [x] [minor] `KernelResourceBudget` (`mnemosyne-core::kernel_budget`):
  registers/thread, shared-mem/block, threads/block with fully-`const`
  occupancy limiters (`blocks_limited_by_{registers,shared_mem,threads}`,
  `OccupancyLimits::blocks_per_unit` minimum). **Not** register allocation —
  GPU compilers assign registers (ADR 0002 constraint 2). Capacities arrive
  as plain quantities (themis `GpuTopology` accessor values) so
  mnemosyne-core stays `no_std`/dependency-free; unreported capacities
  surface as `u32::MAX` "no information", never a fabricated bound.
  Verification: closed-form Ampere-class fixtures, zero-budget/zero-capacity
  semantics, const-evaluability test. Remaining: shared-memory arena
  budgeting (the literal-allocation part) pairs with Stage D1 device pools.

## Closed

- [x] [patch] Stack-interner final-release critical section. The final entry
  and map-key `Arc` values are removed under the owning shard lock but dropped
  only after releasing it, preventing allocator/deallocation work from
  extending or re-entering the lock. Evidence tier: value-semantic and
  concurrent nextest coverage plus focused Criterion measurement.
- [x] [patch] `AlignedVec::into_vec` source-buffer release. Conversion keeps
  the required one-copy boundary into the standard `Vec` allocator and now
  drops the distinct aligned source allocation. Evidence tier: value-semantic
  nextest plus Miri nextest leak checking.
- [x] [patch] Page-metadata provenance and remote-free aliasing. Cached page
  addresses are refreshed through explicit exposed provenance before reuse;
  cross-thread frees mutate only the page-local atomic queue through raw-field
  access and never create an exclusive borrow of owner-managed metadata.
  Evidence tier: Miri under Stacked Borrows and Tree Borrows plus 125
  value-semantic nextest cases.
- [x] [patch] Refresh `mnemosyne-local` to Melinoe 0.9.0 so the allocator and
  scheduler graph resolves one validated executor-capability provider version.

- [x] [patch] Atlas provider graph refresh. `mnemosyne-local` now requires
  sibling Atlas `melinoe` `0.8.0`, and `Cargo.lock` resolves Themis to
  `0.9.17` so downstream Atlas consumers do not see a `melinoe ^0.7.0` versus
  `0.8.0` resolver conflict. Evidence tier: compile-time provider integration.
  Current gates: `rustup run nightly cargo check -p mnemosyne-local`;
  downstream `rustup run nightly cargo check -p kwavers-solver --lib`;
  downstream `rustup run nightly cargo clippy -p kwavers-solver --lib
  --no-deps -- -D warnings`.
- [x] [patch] Eunomia scratch local-source contract. `mnemosyne` and
  `mnemosyne-arena` resolve optional Eunomia support from the sibling Atlas
  checkout and removed the obsolete internal `num-complex` scratch feature after
  auditing the local Atlas consumer surface. Consumers should enable
  `mnemosyne/eunomia` and use `eunomia::Complex`; no remaining local
  `mnemosyne/num-complex` consumer was found. Evidence tier: compile-time
  validation plus value-semantic feature coverage. Current Atlas-checkout gates:
  `cargo check -p mnemosyne-arena --features eunomia`; `cargo nextest run -p
  mnemosyne-arena --features eunomia`; `cargo check -p mnemosyne --features
  eunomia`; `cargo nextest run -p mnemosyne --features eunomia`; package clippy,
  doctests, rustdoc, and no-default build checks for both packages.
- [x] [major] AR-2 WGPU callback registration soundness (superseded by ADR 0003). The public
  `WGPU_{ALLOCATE,DEALLOCATE}_CALLBACK` raw `AtomicPtr<c_void>` statics are now
  private `mnemosyne-backend` slots, and consumers register through the typed
  unsafe `register_wgpu_callbacks(WgpuAllocateCallback, WgpuDeallocateCallback)`
  API. The sibling `hephaestus-wgpu` staging callback registration was migrated
  in the same change set. Evidence tier: type-level function-pointer contract
  plus value-semantic Mnemosyne tests and downstream Hephaestus WGPU gates.
  Verification: focused Mnemosyne fmt/check/clippy/nextest/doctest/rustdoc
  gates and Hephaestus `hephaestus-wgpu` fmt/check/clippy/nextest (129/129).

## Closed

- [x] [major] WGPU callback registration publishes one immutable
  allocate/deallocate pair and rejects conflicting pairs. Concurrent readers
  observe only absent or one complete permanent pair. ADR 0003 subsequently
  removes this backend because WGPU 30 invalidates its pointer contract, so
  the former ADR 0002 record is absorbed into
  `docs/adr/0003-remove-wgpu-raw-pointer-backend.md`.

- [x] [minor] Replace the always-resident 720,896-byte per-CPU cache table with
  a `OnceLock<Box<PerCpuCache>>` handle. The production static now stores only
  the initialization cell; the table is allocated on first explicit cache use.
  `PER_CPU_CACHE` field access remains available through `Deref`. Evidence:
  compile-time handle-versus-table layout assertion, lazy-initialization value
  test, release build, 62 local allocator nextest cases, and warning-denied
  Clippy.

## Open

Filed from the 2026-08-12 verification-posture review:

- [x] [patch] **MN-437 — Miri-reported UB in the reclamation seam.** Fixed.
  Cause: reclamation took `&mut Page` while reaching the parent `Segment`
  through a separately-derived pointer, so the page borrow and the segment
  access sat on different provenances and the segment access invalidated the
  borrow. Stacked Borrows reported it two ways — a wildcard read removing the
  strongly-protected `&mut Page` argument, and a failed two-phase retag of a
  page borrow already popped.
  Fix: the seam is now addressed by `(segment, page_index)` and derives the page
  pointer from the segment, so every access in the module shares one
  provenance. `reclaim_thread_free_in_segment`,
  `reclaim_thread_free_if_present_in_segment` and
  `reclaim_thread_free_for_policy` replace the `&mut self` methods, and
  `set_alloc_count_in_segment` does the same for the occupancy write. The page
  argument was redundant all along — segment plus index determines it, and
  accepting it separately is what let the provenances diverge.
  Evidence: `mnemosyne-memory-core` 18/18 clean under **both** Stacked Borrows
  and Tree Borrows, workspace 282/282, warning-denied Clippy, rustfmt. The crate
  is back in the Miri job under both models.
  Worth recording: the first version of this fix passed Stacked Borrows while
  Tree Borrows still rejected it (the hot-path test used its `&mut Page` after
  the reclaim call had disabled the tag). A single-model gate would have
  certified a still-broken fix, which is why the job now runs both.

- [x] [arch] **MN-438 — segment-addressed page access.** Complete. The
  `&mut Page`-across-segment-access pattern is gone from `mnemosyne-core`,
  `mnemosyne-local`, `mnemosyne-heap` and `mnemosyne-decay`: the reclamation
  seam, `initialize_free_list`, and the three `alloc_count` families are
  segment-addressed, and the allocator's page paths carry `*mut Page` instead of
  minting references.
  The finding that justifies the whole item: `&mut Page` was not merely a
  provenance technicality, it was an **exclusivity claim the allocator does not
  have**. Miri's data-race detector reported the retag at
  `segment/reclaim.rs` — creating `&mut (*curr).pages[i]` — against a concurrent
  non-atomic read of `block_size` in `free.rs` on another thread. Remote threads
  read page metadata during cross-thread free *by design*, so no page there can
  ever be exclusively borrowed. The raw-pointer conversion is correctness, not
  appeasement.
  Evidence: workspace 282/282; `mnemosyne-memory-core` clean under Stacked and
  Tree Borrows; warning-denied Clippy; rustfmt; doctests. `mnemosyne-local`'s
  aliasing UB is eliminated — it now reaches the multithreaded tests, where the
  remaining failures are data races on *segment* metadata, tracked as MN-439.

- [x] [arch] **MN-439 — shared segment metadata is non-atomic.** Done for the
  segment header. The cross-thread free path no longer races with the owner:
  `owner` and `owner_allocator` are atomic with a documented Release/Acquire
  pairing, and `cookie_for`, `cookie_for_dynamic`, `free_list_encrypted` and
  `Page::parent_segment_of` take raw pointers so nothing on that path retags a
  whole `Segment` or `Page`. `AtomicUsize`/`AtomicPtr` match the sizes and
  alignments they replace, so the pinned layout is unchanged.
  The structural half was the important one: a *shared* reference is enough to
  race. `&Segment` retags the entire header, so it conflicts with any concurrent
  field write regardless of which field the reader wanted — which is why an
  accessor taking `&self` reintroduced the race immediately after the field
  itself was made atomic.
  `Segment`'s `unsafe impl Sync` justification was rewritten. It had claimed
  "all non-atomic fields are mutated solely by the proven owner ... so a shared
  reference observes no data race"; both halves were false and Miri contradicted
  them. A safety comment asserting an invariant the code does not hold is worse
  than no comment, because it is what a reader checks against.
  Evidence: `mnemosyne-local` under Miri no longer reports any data race.
  Remaining failures there are MN-440 and two Miri-on-Windows limitations
  (subprocess abort tests calling `CompareStringOrdinal`), not defects.

- [x] [arch] **MN-440 — the re-entrancy guard lives inside the object it
  guards.** Done. The gate is now a `Cell<bool>` sibling of the allocator on
  `LocalAllocatorSlot` and `RawHeap`, checked before any `&mut ThreadAllocator`
  is formed; `ThreadAllocator` lost the field and
  `record_defrag_operation`/`run_periodic_defragmentation` take the state as a
  parameter.
  The token constraint that sank the first attempt was resolved by construction
  rather than by a parallel accessor: `LocalAllocatorSlot` is `#[repr(C)]` with
  `allocator` at offset 0, so the slot address and the allocator address are the
  *same value*. One cached pointer therefore serves as both the segment owner
  token and the gate handle — every `SegmentOwner::matches` comparison sees the
  value it saw before. A const assertion (`SLOT_ALLOCATOR_AT_OFFSET_ZERO`) fails
  the build if that layout invariant ever lapses, so the silent misrouting the
  first attempt would have shipped is now a compile error.
  Two further constraints only Miri surfaced, both worth keeping in mind for any
  similar guard:
  - *Provenance, not just address.* `allocator_ptr` had to be re-derived from the
    slot rather than from `UnsafeCell::get()`. Both produce the same address, but
    a pointer whose provenance covers only the allocator cannot legally reach a
    sibling field past it.
  - *Field projection, not slot reference.* The gate is read through
    `&raw const (*slot).is_allocating`, never through a reconstructed `&Self`.
    Forming the reference would retag the whole slot including the allocator,
    which is precisely the aliasing the gate must be able to run *during*.
  `free.rs`'s owner fast path also stopped consulting `alloc.is_current_segment`
  and now reads `(*segment).is_current` — the owner's own mirror of the same
  fact, already the established form in `occupancy` and the cold free path. That
  path runs with the gate raised, so answering the question through the allocator
  required a borrow the gate exists to forbid.
  Evidence: `unguarded_fast_path_rejects_reentrant_borrow` and
  `reentrant_current_segment_local_free_uses_metadata_fast_path` pass under Miri;
  workspace 282/282; clippy `-D warnings` clean.
  Acceptance **not yet met**: `mnemosyne-local` still cannot join the Miri job,
  but no longer because of this item. With the gate fixed the run gets far enough
  to reach two pre-existing defects that previously hid behind it (MN-442,
  MN-443). Those are now the blockers for this acceptance and for MN-439's.

- [x] [major] **MN-442 — the huge-allocation classifier read uninitialized
  page metadata.** Done. `usable_size` and the `thread_free` classifier both
  branched on `pages[page_index].block_size` to decide whether an allocation was
  huge, *before* checking the index. When a huge allocation's alignment request
  lands its payload on a segment boundary (`align >= SEGMENT_SIZE`, 2 MiB),
  `locate_segment` masks the user pointer down to that boundary — an address
  that holds the caller's payload, not a segment header. The classifier was
  reading user bytes as page metadata.
  Both sites now test `page_index == 0` first. Page 0 is segment metadata and is
  never allocated from, so a zero index unambiguously means "segment-aligned,
  therefore large/huge" and routes to the metadata slot without touching a
  header that may not exist.
  This was not merely a Miri complaint. The regression test
  (`usable_size_ignores_payload_bytes_at_a_segment_aligned_huge_pointer`) fills
  a segment-aligned 4 MiB payload with `0xAB` and, with the fix reverted,
  `usable_size` returns 12,370,169,555,311,111,083 for a 5,570,560-byte mapping
  — the fill pattern read as a size. A caller sizing spare capacity from that
  writes arbitrarily far past the mapping. The assertion is bounded by the
  mapping end, not by the request: the over-report is the dangerous direction,
  and an earlier `>= request` form passed with the bug present.

- [x] [major] **MN-443 — forbidden wildcard write in page occupancy under Tree
  Borrows.** Done. The `&mut self` occupancy wrappers recovered their segment by
  masking the receiver's address, producing wildcard provenance; their own
  rustdoc already recorded that using the receiver afterwards was UB. They were
  MN-438 residue kept for unconverted callers — but by then every *production*
  caller had been converted, and only five test sites remained. The wrappers are
  deleted and those sites use the `_in_segment` forms.
  Two follow-on aliasing fixes fell out, both the same shape: a caller holding a
  reference across a segment-addressed write. Four sites reached page metadata
  through `(*segment).pages.as_mut_ptr()`, which materializes a `&mut` to the
  whole pages array and is invalidated by the very writes those helpers perform
  through the segment's provenance. They now use `&raw mut (*segment).pages[i]`,
  a pure place projection that creates no reference — one of them in production
  code (`realloc.rs`), the rest in tests.
  Evidence: `mnemosyne-local` runs 57 passed / 0 failed under Stacked Borrows,
  where before MN-440 it aborted at the first UB. Each Tree Borrows rejection
  was observed and fixed in turn — deleting the wrappers moved the failure off
  `occupancy.rs:46` onto the `as_mut_ptr` sites, which then moved off those —
  and the confirming Tree Borrows pass has since come back clean: no Undefined
  Behavior reported under either model. Adding `mnemosyne-local` to the Miri job
  is now blocked only by MN-445 (two integration tests), not by aliasing.
  Also drained the global huge pool at the end of the three tests that allocate
  huge mappings directly. The pool retains released mappings for reuse by
  design, which Miri's leak checker reports at exit; the arena and cross-thread
  tests already purge for the same reason, so leak checking stays on rather than
  being suppressed for the new job.

- [x] [patch] **MN-445 — mnemosyne-local joins the Miri job.** Done. Both
  blockers turned out to be real defects rather than environment.
  The failure was `topology_tests::test_per_cpu_cache`, and the cause was a
  genuine bug: `try_free_cpu` and `try_alloc_cpu` ran `compare_exchange_weak`
  inside a two-round loop whose budget exists for CPU migration, so a spurious
  failure spent that budget and reported the cache unavailable when it was
  empty and uncontended. Live on aarch64; invisible natively because x86 lowers
  weak and strong alike. Both use a strong exchange now.
  The timeout was `test_per_cpu_cache_contention_bounds` — a spinner thread
  against a thousand allocation attempts, sized for native execution, now
  skipped under Miri with the progress-under-contention property left to loom
  (MN-433).
  Three further Tree Borrows timeouts surfaced once those cleared, each fixed at
  its cause rather than by shrinking coverage: byte-at-a-time verification
  replaced with bulk fill/compare (keeping the byte walk for failure offsets);
  an orphan-adoption test re-sized to a large small-class, since preserved
  per-page keys decode identically in every class; and a 2000-round churn test
  Miri-scoped to one full lcm(8, 19) = 152 cycle. That last one exposed MN-447.
  Final cost, which also settles the scoping question this item carried: 36s
  Stacked and 463s Tree Borrows, both per PR, no schedule split needed. The
  earlier "14x, about ninety minutes" figure came from `cargo miri test`, which
  runs serially; CI uses nextest, and the fixes removed the cost problem
  outright.
  `smallest_class_page_saturates_without_duplicate_or_early_refill` no longer
  crosses the 300s slow mark; see the note under MN-452.
  Both steps ran with `-Zmiri-ignore-leaks` at first; MN-444 removed it, so
  they now carry the same flags as the arena and core steps.

- [x] [patch] **MN-446 — decay purger lifecycle synchronization.** Closed
  2026-08-15. The shutdown wait now tracks a monotonically increasing worker
  generation and the generation's final-exit publication; releasing the
  `SPAWNED` claim during the cancellation/restart handshake is not sufficient
  evidence of shutdown. The focused decay regressions cover value-semantic
  timeout behavior and concurrent restart progress, while the purge tests
  retain their reclamation assertions. Exact head `dce730789a22adc4e39c513015c0a3b36ad1934b`
  passed [PR #54 CI run 31870326847](https://github.com/ryancinsight/Mnemosyne/actions/runs/31870326847):
  Rust verification (formatting, strict Clippy, workspace nextest, and
  doctests), Loom, and Miri all passed. Local `--locked` Cargo gates remained
  blocked by the preserved Atlas-overlay-derived peer `Cargo.lock`; the
  lockfile and ADR 0003-0006 were not part of the provider change.

- [x] [patch] **MN-447 — no test drove page recycling.** Done.
  `emptied_page_is_recycled_into_another_size_class` and its hardened sibling
  now fill pages until the allocator moves to a second segment, empty the first
  segment, and allocate a *different* size class — which can only be served by
  popping one of the emptied pages and re-initializing it.
  Two things had to be right for the test to reach the transition at all. A page
  only leaves the active list once its segment is no longer being sliced
  (`free.rs` keeps it in place while `Segment::is_current` holds), which is
  exactly why ordinary churn never got there: eight live blocks never leave the
  first segment. And filling a page had to be cheap, so it uses the largest
  small class — eight blocks per page instead of the 4096 a 16-byte class needs
  — which is what keeps the whole thing inside the Miri budget.
  Recycling is proven by address as well as by counter: a fresh page would come
  from the *current* segment, so a block landing back in the first segment can
  only have come from a page emptied there, and the fill loop's own bound fails
  the test if the allocator never left that segment. That argument needs no
  stats, which is what makes the hardened case testable (see MN-448).
  Verified non-vacuous by disabling the empty-list branch in `get_new_page`:
  both tests fail, including the hardened one that rests solely on the address
  argument. Counter deltas (`recycled_pages`, `recycle_sweeps`,
  `fresh_segments`) are asserted as deltas rather than absolutes, so a sibling
  test dirtying the thread's allocator cannot satisfy them accidentally.
  Segment reclamation is now covered too, in `reclaim_tests.rs`. The reason
  recorded here for leaving it out was wrong: reclamation was said to run on
  allocator drop, after any assertion could observe it. `reclaim_owned_segments`
  is a `pub` method — a test constructs an allocator, calls it, and then looks.
  The branch worth testing is the one choosing a segment's sink. An emptied
  segment goes back to the pool for reuse; one still holding live blocks cannot
  be unmapped at all, because the pointers its former owner handed out are still
  in use, so the orphan pool is its only destination. Choosing wrong there hands
  a use-after-free to whoever still holds a block, which is why the live case
  asserts the block contents survive both the reclaim and a second allocator's
  adoption of the segment.
  Both are independently non-vacuous, checked by inverting the sink condition in
  each direction: forcing live segments down the deallocate path fails the
  orphan test and not the pooled one, and forcing every segment to orphan fails
  the pooled test and not the orphan one. Neither assertion is doing the other's
  job.
  The blocks are freed through `thread_free`, which sees an owner-token mismatch
  against a locally constructed allocator and routes them to the page's atomic
  queue — the same path a foreign thread takes — so reclamation also drains that
  queue on the way to deciding the count is zero.

- [x] [major] **MN-448 — allocator stats ignored the policy and reported the
  wrong allocator.** Done, per ADR 0008.
  `thread_allocator_stats` and `memory_stats_generic` now take the allocation
  policy as a generic parameter and route through
  `with_allocator_for_policy`. Previously both used the policy-blind selector
  and always read the standard TLS slot, so an application running
  `MnemosyneAllocator<HardenedPolicy>` received a near-empty snapshot of a
  cache it had never allocated through — plausible enough to be read as real.
  `memory_stats()` keeps its signature and supplies `StandardPolicy`, mirroring
  `Mnemosyne` being `MnemosyneAllocator<StandardPolicy>`'s shorthand.
  Rejected a `_for_policy` twin beside each existing function: that is the
  parallel-API defect, and it would have left the original names still
  answering for the wrong allocator behind better documentation.
  Checked that the neighbours do not share the bug — `purge_generic`,
  `reset_generic` and `decay` act on the per-backend segment pool and the decay
  thread, neither keyed by policy, so they stay policy-blind correctly.
  `cargo-semver-checks` confirms the `[major]` classification rather than it
  being asserted: `function_requires_different_generic_type_params` on both
  entry points, "semver requires new major version".
  Non-vacuity: reverting the selector to the policy-blind form fails both
  `allocator_stats_report_the_policy_the_caller_asks_for` (hardened allocations
  must move the hardened counters and leave the standard ones alone) and
  MN-447's hardened recycling test, which can now assert the same counter
  deltas as its standard sibling — this item's stated acceptance.

- [x] [major] **MN-451 — `ensure_options_initialized` has one export path.**
  Done. It was `pub use`d at the crate root *and* re-exported from `internal`,
  one concept reachable two ways. It now lives only in `internal`, beside the
  seams MN-450 moved there, which is the honest home: its callers are the
  `impl_local_allocator_selector!` expansion and `mnemosyne-heap`, and neither
  is a consumer calling an entry point.
  The blast radius in this item's own filing was overstated. I wrote that
  collapsing it meant "recompiling every consumer" of the macro, implying churn
  at call sites. There is none: `$crate` resolves inside the defining crate, so
  changing what the macro expands to leaves every invocation untouched, and
  `mnemosyne-heap` already imported from `internal`. The change is three lines
  in `lib.rs`.
  Reclassified `[patch]` to `[major]` on evidence rather than assumption:
  `cargo-semver-checks` reports `function_missing: pub fn removed or renamed`
  for `mnemosyne_local::ensure_options_initialized`. It rides an unreleased
  cycle already major from MN-440 and MN-448, so it owes no bump that was not
  owed already; its other three findings belong to those items.
  Recorded in CHANGELOG under Unreleased, with the note that macro users need
  no edit.

- [x] [patch] **MN-452 — the saturation test's slow margin.** Done, and the
  reasoning that closed MN-445 with this left open was wrong twice over.
  It was recorded as irreducible: saturating a 16-byte-class page is 4096
  allocations, and that count is the property. Timing the test *in isolation*
  gave 66s, not the 300s+ the suite reported. Two separate things were going
  on. Nextest runs one Miri interpreter per core — 24 on this machine — so a
  test's reported wall clock is its own work plus contention with 23 others,
  and the suite number was never measuring the test. And half the test's own
  cost was teardown: rebuilding the page's free chain one `set_next` per block,
  after every assertion had already been made.
  That teardown bought nothing. Reclamation decides a page's fate from
  `alloc_count` and never reads the chain; the segment is released immediately
  after; and a page that gets pooled rather than released has its free list
  rebuilt by `get_new_page` before anything allocates from it. Dropping it took
  the test to 36s and the whole Tree Borrows suite from 463s to 172s — it was
  the long pole in a parallel run.
  Nothing was shrunk: the 4096 allocations, every assertion, and the
  saturation-and-refill transition are all unchanged. What went was cleanup
  work that asserted nothing.
  Worth keeping in mind for the next slow marker here: under this much
  parallelism the suite's per-test number is work plus contention, and only an
  isolated run distinguishes them.

- [x] [patch] **MN-450 — the test-only export is out of the entry-point
  surface.** Done, and two claims in the original filing were wrong.
  It said the item shows up in the crate's rendered docs. It does not:
  `reset_options_for_testing` was already `#[doc(hidden)]`, as was
  `mark_options_initialized`. I asserted that without checking. What was
  actually true is narrower — both sat in the crate root's `pub use` list
  beside the real entry points, so the surface *read* as though they belonged
  there even though rustdoc never rendered them.
  It also called removing the export breaking. `cargo-semver-checks` disagrees:
  it does not treat `#[doc(hidden)]` items as public API and reports no new
  failure for this change. The three it does report are MN-440's, already under
  Unreleased. So this is `[patch]`, not `[major]`.
  What changed: both moved into the crate's own `#[doc(hidden)] pub mod
  internal`, which is where `mnemosyne-heap` already reaches for sibling
  internals, leaving the root list to items a consumer calls. Each definition
  now records why it is `pub` at all — the tests needing the reset are
  integration tests in `mnemosyne-decay`, `mnemosyne-heap` and
  `mnemosyne-prof`, which are separate crates, so neither `#[cfg(test)]` nor
  `pub(crate)` reaches them — and `mark_options_initialized` is documented as
  the seam behind `mnemosyne::configure` rather than a consumer call.
  `ensure_options_initialized` stays at the root: the selector macro expands to
  `$crate::ensure_options_initialized()`, and `mnemosyne-heap` calls it from
  production code. It is exported from both the root and `internal`, which is a
  duplicate path worth collapsing, but doing so means editing the macro's
  expansion and every consumer of it — filed rather than folded in here.
  311/311 workspace tests, 7/7 doctests, clippy `-D warnings` clean.

- [x] [patch] **MN-449 — `mnemosyne-local` ships doctests.** Done for the
  consumer entry points: `thread_alloc`, `thread_alloc_layout`, `thread_free`,
  `thread_free_layout`, `thread_realloc`, `usable_size` and
  `thread_allocator_stats` each carry a runnable example. Seven doctests where
  the crate had none.
  They exercise contracts rather than demonstrate syntax, and each `# Safety`
  precondition appears as a `// SAFETY:` note on the example's own unsafe
  block, so a reader meets the obligation where they would have to discharge
  it: an invalid request returns null instead of panicking; a block is freed
  exactly once and freeing null is a no-op; a layout-taking free must be given
  the allocation's own layout; growing preserves every byte; and `usable_size`
  reports what may be dereferenced rather than what was asked for, with the
  whole reported span writable.
  The `thread_allocator_stats` example is load-bearing rather than
  illustrative: it asserts that asking under a policy you did not allocate with
  reports zero, which is ADR 0008's contract in executable form. Reverting the
  selector to its policy-blind version fails that doctest, so it guards the fix.
  Not covered, deliberately: `LocalAllocatorSelector` / `LocalAllocatorSlot`
  are the backend seam, implemented through the selector macro rather than
  called, and `ensure_options_initialized` / `mark_options_initialized` are
  initialization plumbing. Neither is something a consumer writes against.

- [x] [patch] **MN-444 — Miri leak checking is on for mnemosyne-local.** Done,
  and by removing the exclusion rather than narrowing it.
  The premise the exclusion rested on was wrong. Both this item and the CI
  comment claimed nearly every test in the crate leaves segments retained, so
  leak checking would amount to asserting that a cache must be empty. Running
  Miri without `-Zmiri-ignore-leaks` showed 6 of 75 tests leaking; the other 69
  already passed the leak gate. The claim was never measured before it was
  written down.
  Five of the six were cross-thread or local-`ThreadAllocator` tests ending with
  segments in a pool. `drain_orphan_pools_for_test` already did exactly the
  right drain — orphan pools for both backends, then both segment pools, which
  also purges the huge pools — but it was only ever called at the *start* of
  three tests, for a clean slate, never at the end. It now lives in the shared
  `fixtures` module as `drain_all_pools`, behind a `PoolDrain` RAII guard that
  the six tests declare right after their `TEST_LOCK` guard, so locals drop
  first (returning segments to the pools), the drain runs, and only then is the
  lock released.
  The sixth, `hardened_policy_detects_freelist_tamper`, was a genuine case: it
  deliberately poisons a page's free list, so the allocator can neither serve
  from that page nor reclaim its segment, and the segment was still held at
  exit. It now repairs the free-list head and frees its outstanding block
  *after* the assertion, which changes nothing it measures — every observation
  is made against the tampered state — and lets the ordinary reclaim path
  release the segment.
  Both `Miri — local` steps now run with the same flags as the arena and core
  steps; nothing in the job is exempt from leak checking. 75/75 under both
  borrow models: 40s Stacked, 287s Tree Borrows, the latter down from 482s
  because the drained memory is no longer tracked to exit.

- [x] [patch] **MN-441 — `is_current` is owner-only by construction.** Done, by the second of the two
  options: documented and *enforced* as owner-only rather than made atomic.
  The audit settles the question the item left open. The only remote-thread
  path into a segment is `thread_free_cold`, which pushes into the page's
  `AtomicFreeList` and touches nothing else in the header. Every reader of the
  flag — the occupancy transitions, the local free fast path, the
  defragmentation sweep — runs on the owning thread, most already holding
  `&mut ThreadAllocator`. So the flag needed enforcement, not synchronization:
  an atomic would sit on the small-free fast path and advertise a cross-thread
  contract that does not exist, inviting the very access the protocol forbids.
  The field is now private, reached only through `Segment::is_current` /
  `Segment::set_current`, which take raw pointers and project to the single
  byte — the same shape MN-439 gave `owner`/`owner_allocator`, and for the same
  reason: a `&self` accessor retags the whole header and races with concurrent
  writes to any other field, which is exactly what Miri caught against this
  flag before.
  Enforcement is structural, not a runtime ownership check. Having the
  accessors `debug_assert!` the caller's owner token was considered and
  rejected: `mnemosyne-core` cannot see TLS owner identity without inverting the
  layering, and `occupancy.rs` holds no token to pass.
  Bookkeeping note: this entry sat as `- [ ] status=in-progress owner=claude`
  from 2026-08-14 until now, with a body reading "Done". The work landed in
  e421584 and merged; only the marker and the claim were left stale. A claim
  that outlives its work is worse than no claim — the stale-claim sweep reads
  it as a live hold on `segment.rs`, `occupancy.rs`, `free.rs`,
  `local_alloc.rs`, `reclaim.rs` and `raw_heap.rs`, so a peer looking for work
  would have skipped that whole scope rather than taken it.
  Also recorded the invariant the flag carries at a pool boundary, which the
  item did not name: a segment must never reach a global pool with the flag set,
  or the next thread to claim it inherits a stale "currently being sliced" state
  and skips occupancy bookkeeping. `reclaim_owned_segments` upholds it by
  clearing the current segment before walking the owned chain and clearing the
  flag again on every orphaned node; the defragmentation sweep skips the current
  segment outright.
  Privatizing the field caught five test sites building `Segment` by struct
  literal with `..zeroed()`, bypassing `Segment::initialize` entirely — so their
  page array and key schedule stayed zeroed, and `..zeroed()` would silently
  absorb any field added later. They now run the real initializer.
  Evidence: workspace 292/292 and clippy `-D warnings` clean at the
  code-complete revision; `cargo fmt --all --check` clean; Miri on
  `mnemosyne-memory-core` 18/18 under both Stacked and Tree Borrows, which is
  the package the accessors and the occupancy readers live in. The later
  `mnemosyne-decay` lifecycle failure was tracked and closed as MN-446.

- [x] [patch] **MN-454 — `mnemosyne-backend` compiles and passes under Miri.**
  Done. `backends/unix.rs` gated the `SEGMENT_SIZE` import on
  `all(target_os = "linux", not(miri))` while two tests used the constant
  unconditionally, so `cargo miri` on Linux failed to build the test list.
  Both are hugepage-hint tests, and the hint path they cover is itself
  `not(miri)`, so they now carry the same gate as the code they exercise —
  under Miri they would have had no subject either way.
  Verified the way this item asked for, by compiling rather than reading, and
  without a Linux host: cross-checking the tests for
  `x86_64-unknown-linux-gnu` under `--cfg miri` reproduces the exact CI error
  (`E0425` at unix.rs:292) and then shows it clean. Also confirmed the ordinary
  Linux build still compiles those tests, so the gate did not delete coverage
  where it belongs, and that the host Miri run still passes 13/13.
  That cross-check is the tool this whole episode was missing. A Windows host
  compiles the Windows backend, so a local Miri pass on this crate says nothing
  about the unix path — `RUSTFLAGS="--cfg miri" cargo check --all-targets
  --target x86_64-unknown-linux-gnu` closes that gap for a compile-time failure
  without waiting on CI.

- [x] [major] **MN-455 — the pool stack's node link is atomic.** Done.
  `Segment::next_free_segment` is now an `AtomicPtr<Segment>`, which removes the
  race without giving up the cleared-link invariant three tests pin. Every
  access is `Relaxed`, and that is not a shortcut: the link carries no
  happens-before obligation of its own, because publication and observation are
  the head CAS's `Release`/`Acquire`, so a node reached through the head already
  synchronizes with the push that linked it. The atomic is for the absence of a
  race, not for ordering — recorded at the field.
  The proof is the model that found it: `concurrent_pops_never_hand_out_the_same_node`
  is un-ignored and passes, where before it reported loom's causality violation.
  All three pinned assertions survive, now reading through the atomic, including
  `huge_pool_concurrent_push_pop_conserves_every_segment`'s "still has a dangling
  next link".
  `cargo-semver-checks` classifies it, as the acceptance asked: 5 major checks,
  `struct_pub_field_missing` among them. It rides an unreleased cycle already
  major.
  One scare worth recording: two arena Miri tests failed locally afterwards on a
  memory leak. Checked against HEAD in a throwaway worktree rather than assumed
  — they fail identically without this change. It is the known Windows-only
  huge-pool retention leak, and CI on Linux passes them.

- [ ] [major] **MN-436 — preserve allocator mapping provenance.**
  status=in-progress; integrator=codex; lease=codex on
  `cross_thread_tests.rs`, this entry, and the owner-local checklist through
  the fix-forward commit;
  last-update=2026-08-27. ADR 0009 replaces exposed/wildcard provenance in
  core, arena, and local allocation paths with mapping-derived raw pointers,
  `AtomicPtr` packed heads, and `map_addr` tagging. Unsound page receiver APIs
  are removed and every in-tree caller is migrated to raw segment/page
  associated functions.
  Acceptance evidence: strict-provenance Miri passes core 18/18, four focused
  tagged-stack pointer-pack tests, and the exact Leto Mnemosyne constructor and
  drop path; native nextest passes 171/171; affected Clippy, doctests, and
  warning-denied rustdoc pass. SemVer checks classify only core as the expected
  major break; arena and local pass 196/196. The native-sized arena concurrency
  binaries remain out of Miri because the interpreter exceeds the runtime
  bound; unchanged real workloads pass nextest and remain covered by hosted
  TSan run 32198740189 and loom models. The distinct `RawHeap` address recovery
  is outside this core/arena/local increment and is tracked by MN-456.
  Hosted PR #75 Miri then found one test-only contract violation: run
  `33112953132` created page pointers through `&mut (*segment).pages[index]`,
  whose subobject retag cannot authorize the list operation's subsequent
  parent-segment occupancy write. Production page-list callers already use raw
  place projections. Fix forward with the same mapping-derived projection and
  require both borrow models before closure.

- [ ] [major] **MN-456 — preserve `RawHeap` mapping provenance.**
  status=todo; integrator=unclaimed; dependencies=MN-436; scope=
  `crates/mnemosyne-heap`, top-level heap consumers, focused tests and docs;
  non-goal=changing allocation policy or benchmark workloads. Replace integer
  segment recovery in `RawHeap` with allocation-derived pointers and migrate
  any public surface required by the proof. Acceptance: strict-provenance Miri
  exercises heap allocate/free/reallocate and drop through value-semantic tests,
  native nextest remains within committed bounds, SemVer class is measured,
  and no exposed-provenance reconstruction remains in the heap path.

- [x] [arch] **MN-433 — concurrency model checking.** Done; the seam and all
  the planned models exist and gate in CI.
  Delivered: `mnemosyne_core::loom_shim` re-exports loom's atomics under
  `cfg(loom)` and `core`'s otherwise, and `AtomicFreeList` imports through it,
  so models drive the shipped code rather than a transcription that could drift.
  `cfg(loom)` rather than a cargo feature on purpose — features are additive and
  could unify instrumented atomics into a real build; a cfg set only by
  `RUSTFLAGS` cannot. `Page::new`/`AtomicFreeList::new` keep their `const` form
  in ordinary builds and gain non-const twins under `cfg(loom)`, because loom's
  atomics are not const-constructible.
  Three models for the free queue's head protocol: concurrent pushes are not
  lost, a push concurrent with a drain is observed exactly once (losing it leaks
  a block, double-observing it double-frees), and a drain resets both the packed
  address and the count. A `loom` CI job runs them.
  **The models were confirmed non-vacuous**: replacing the CAS with a plain
  store makes loom find the lost-push and swallowed-push interleavings, so the
  assertions can fail. That check matters — the suite completes in well under a
  second, which on its own looks indistinguishable from exploring nothing.
  Second increment landed: the segment ownership protocol is modelled.
  It needed a structural change first. `Segment` embeds
  `[Page; PAGES_PER_SEGMENT]`, each with an `AtomicFreeList`, so under
  `cfg(loom)` building one segment creates one instrumented atomic per page and
  the state space explodes — the ownership pair was unmodellable while it lived
  inside `Segment`. It is now a `SegmentOwnership` type holding both fields with
  private members, which a model *can* build, so these models drive the shipped
  code rather than a transcription. The extraction stands on its own merits too:
  the two fields are only meaningful together, and holding them as independent
  members is what made a torn identity expressible at all.
  Four models, two of which fail if the publishing store is weakened to
  `Relaxed`, so they test the Release/Acquire edge rather than exercising it:
  observing an owner implies seeing what that owner published before claiming;
  observing an orphan implies seeing the teardown; the pair converges after a
  claim; and no reader observes a value that was never published.
  **A finding worth keeping**, because the first version of these models
  asserted it as a bug and loom was right to reject them: the owner and the
  allocator are two stores, so a reader landing between them sees a mixed pair.
  No ordering fixes that — only a single atomic location or a lock would. It is
  harmless today because the free path reads the allocator only after matching
  the owner against its *own* token, and a thread that matches is the one that
  wrote both. It stops being harmless the moment a reader compares the owner
  against anything else, which is worth remembering before writing such a
  reader.
  Third increment: the page publish/reclaim pair and the pool stack are
  modelled, which was everything left on this item.
  The page layer's own concurrency is the check-then-drain in
  `reclaim_thread_free_if_present_in_segment`: it tests `is_empty` and only then
  drains, so a remote free landing in between is deferred to the next reclaim.
  The invariant is conservation, not immediacy — the block is drained now or
  still queued after, never neither, since neither would be a leak. Non-vacuous:
  making `is_empty` consume the queue fails that model and only that one. The
  page's `alloc_count` arithmetic is deliberately unmodelled; it is written only
  by the owner, so there is no interleaving to explore.
  `TaggedSegmentStack` was the lowest-priority model on the grounds that it
  would confirm a written argument rather than probe an unexamined one. It did
  not: modelling the pop protocol found a real data race, now MN-455. That is
  the argument for doing the low-priority ones — a written argument is exactly
  the kind that stays convincing after the code stops matching it.
  Its publication model passes and gates in CI. The race reproduction is kept
  runnable and `#[ignore]`d against MN-455 rather than deleted.

- [x] [patch] **MN-434 — the SeqCst audit.** Done, and the premise was wrong:
  this crate has **zero** `SeqCst` in shipped code. All 53 counted occurrences
  are test code — drop counters, mock-backend counters, profiler test counters —
  where `SeqCst` costs nothing and proves nothing. The filing counted raw
  occurrences and assumed a hot allocator path.
  Audited what is actually there instead: 237 explicit orderings, 117 `Relaxed`,
  56 `Acquire`, 54 `Release`, 10 `AcqRel`. The lock-free cores follow the
  canonical Treiber shape with correct pairing — `Release` on the publishing
  CAS, `Acquire` on the pop and on the `swap` that takes a whole list,
  `Relaxed` on the speculative head load the CAS re-validates, `Relaxed` on CAS
  failure — in both `tagged_stack` and `AtomicFreeList`. The subtle case, the
  failure ordering, already carries its reasoning at the site. Everything
  `Relaxed` is a counter, a flag, or a tuning value, several with their skew
  tolerance written down. Nothing was over-ordered and, more to the point,
  nothing was under-ordered, which is the direction that would actually bite.
  So no site changed, and the acceptance's condition — "no site is relaxed on
  reasoning alone" — is met by there being nothing to relax.
  The finding is worth more than the non-change: `SeqCst` is now a ratcheted
  conformance class stack-wide (`seqcst_production`), counted in production
  source only and baselined per repo, so a new default-`SeqCst` has to be
  justified. The stack has 126, concentrated exactly where ordering matters
  most — moirai 101, melinoe 13, kwavers 10, ritk 2. Filed upward as a moirai
  concern rather than a mnemosyne one.
  Caveat worth keeping: this audit read code on an x86-64 host, which hides
  acquire/release mistakes that aarch64 exposes. MN-435's ARM job is what would
  turn "looks right" into evidence.

- [x] [patch] **MN-453 — TSan covers the global-allocator path.** Done, and the
  exclusion turned out to be unnecessary. The job now includes
  `mnemosyne-memory`, whose integration tests install the allocator
  process-wide with `#[global_allocator]` (PR #63, run 32198740189).
  The reasoning that scoped it out was a guess: that an allocator swapped under
  the sanitizer's own bookkeeping would fight it, since TSan allocates shadow
  state through the same libc surface the shim replaces. Measuring it took one
  line and one CI run, and it is clean — TSan's shadow allocation and Rust's
  `GlobalAlloc` sit at different layers and do not collide here. The guess cost
  real coverage for exactly as long as it went unchecked.
  Verified the pass is not vacuous: 196 tests across 17 binaries, up from 171,
  with `mnemosyne-memory` compiled under `-Zsanitizer=thread` and the
  `global_alloc_tests` binary among those that ran. A green job proves nothing
  if the crate contributed no tests.
  Lane note: this ran in the `fix/gitattributes` worktree, re-pointed rather
  than opened as a third tree — it was clean, idle, and its one commit
  duplicated work already on main. Lane retired and branch deleted on merge.

- [x] [patch] **MN-435 — aarch64 and ThreadSanitizer jobs.** Done; both run in
  CI and both passed on their first execution (run 32183974171).
  `test-aarch64` runs the workspace suite on a native `ubuntu-24.04-arm`
  runner: 317 tests. This is what settles the caveat MN-434 closed with. That
  audit read the segment pool's Treiber loops and the cross-thread free queue
  on an x86-64 host, whose memory model is strong enough that a load which
  should have been `Acquire` usually behaves like one anyway — so the audit was
  a code review, not evidence. Those paths now execute against a weak memory
  model. The job also covers the `cfg(unix)` madvise and hugepage-hint paths
  and the `SEGMENT_ALIGN` arithmetic on a kernel that need not use 4 KiB pages.
  `test-tsan` runs 171 tests across `mnemosyne-memory-core`, `mnemosyne-arena`
  and `mnemosyne-local` under ThreadSanitizer, with no report. Loom proves the
  interleavings it models and Miri checks aliasing; neither watches real
  threads race on real hardware over the pools, the orphan hand-off, or the
  cross-thread queue.
  Verified the TSan job is not vacuous rather than trusting its green:
  `-Zbuild-std` genuinely rebuilt `core`, `alloc`, `std`, `compiler_builtins`,
  `panic_abort`, `panic_unwind` and `proc_macro` from `rust-src` under
  `-Zsanitizer=thread`, 36 crates in 32s. Without that, std is uninstrumented
  and TSan reports it as false positives — a job that would have looked green
  for the wrong reason.
  Scope not taken: TSan excludes the `mnemosyne` crate, whose integration tests
  install the allocator process-wide with `#[global_allocator]`. Tracked as
  MN-453. ASan is also not added — the item allowed "a TSan job", and the
  memory errors ASan finds are the ones Miri already covers here with more
  precision, while the data races it does not find are TSan's job.



Filed from the 2026-07-13 allocator safety, memory, structure, and contention
audit, in priority order:

- [x] [patch] status=done owner=codex scope=`Cargo.toml`, the affected package
  manifests under `crates/`, `Cargo.lock`, `backlog.md`, `checklist.md`, and
  `CHANGELOG.md`; last-update=2026-07-25.
  Restore publishable version requirements for the local Themis, Eunomia, and
  Melinoe workspace paths introduced by commits `6a4bad7` and `1070417`.
  Acceptance met: every direct path dependency in the affected packages has a
  published-version requirement; locked metadata and all focused native gates
  pass; package archive preparation is attempted and is blocked only by the
  absent `eunomia`/`melinoe` registry packages. Provider code is unchanged.

- [ ] [patch] status=blocked owner=external scope=`eunomia`, `melinoe`, and
  `themis` provider publication metadata; last-update=2026-07-25. Publish the
  provider crates and replace Themis's optional git Melinoe edge with the
  canonical provider source. Re-open when crates.io contains `eunomia` and
  `melinoe` at the required versions and Themis has a released source-aligned
  dependency graph. Mnemosyne archive preparation then becomes a local gate.

- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-benchmarks/benches/allocator/workers.rs` and `crates/mnemosyne-local/src/local_alloc/tests/`; last-update=2026-07-25. Restored workspace rustfmt cleanliness without changing benchmark or allocator behavior. Acceptance met: `cargo fmt --all --check`, `git diff --check`, focused nextest 216/216, and the affected benchmark-target compile pass.

- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-heap/src/heap.rs`, `crates/mnemosyne-heap/src/tests/`, and matching PM entries; last-update=2026-07-24. Compute the branded block's runtime layout before `drop_in_place` in `Heap::free`; the current post-drop `size_of_val` reference violates the initialized-value lifetime required by the unsafe operation. Acceptance met: sized and unsized branded frees retain drop counts and release the allocation, the free path has no post-drop reference, and focused heap formatting, warning-denied Clippy, nextest 56/56, four runnable plus six compile-fail doctests, and rustdoc pass. No performance claim.

- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-heap/src/branded_vec.rs`, `crates/mnemosyne-heap/src/tests/`, and matching PM entries; last-update=2026-07-24. Removed `unreachable_unchecked` from the fallible vector growth layout path and return `Err(())` if the repeated layout calculation ever fails. Acceptance met: no production `unreachable_unchecked` remains in the Mnemosyne heap/facade tree, the vector growth contract stays value-correct, and focused heap formatting, warning-denied Clippy, nextest 54/54, doctests, and rustdoc pass. No performance claim.

- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-heap/src/{heap.rs,tiered_heap.rs}`, `crates/mnemosyne-heap/src/tests/`, and matching PM entries; last-update=2026-07-24. Validate the caller-supplied source `Layout` at the safe branded realloc boundary before raw copy/reuse decisions. Acceptance met: oversized or misaligned source layouts return a typed source-layout failure without allocation, mutation, or source loss; valid branded, tiered, zero-size, and vector realloc paths remain value-correct; package check, warning-denied Clippy, focused heap nextest 55/55, doctests, rustdoc, and semver pass. No performance claim. Workspace locked nextest rerun is blocked by concurrent peer manifest/lockfile version drift; the preceding workspace gate was 282/282.

- [x] [major] status=done owner=codex scope=`crates/mnemosyne-heap/src/{heap.rs,raw_heap.rs,branded_vec.rs,tiered_heap.rs}`, `crates/mnemosyne-heap/src/tests/`, `crates/mnemosyne/src/lib.rs`, and matching PM entries; last-update=2026-07-24. Replaced branded realloc's old-layout fallback and source-block loss with validated layouts plus typed `ReallocError` / `TieredReallocError` results that retain the source block and tier through raw, tiered, and vector paths. Acceptance met: invalid requests never enter raw allocation, failure returns the original block/tier, successful and zero-size contracts remain value-correct, and focused plus workspace gates pass. Public return-type break is documented for the pending pre-1.0 version integration. Evidence: focused heap nextest 53/53, workspace nextest 282/282, warning-denied Clippy, doctests/rustdoc, and semver checks. No performance claim.

- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-prof/src/{lib.rs,sampler/{mod.rs,store.rs},tests.rs}`, `crates/mnemosyne-benchmarks/benches/allocator/{profiler,mod}.rs`, `allocator_bench.rs`, and profiler PM entries; last-update=2026-07-15. Replace `mnemosyne-prof`'s global active-sample RMW with per-shard occupancy flags and stop allocating empty maps on remove.
  The pre-change source audit identified pointer-modulo sharding as a separate
  candidate. The real single-thread leak-detector row was 1.0797 us median
  [1.0731, 1.0877] us; Windows flamegraph capture is blocked by the
  administrator-only profiler backend (`NotAnAdmin`).
- Baseline increment 2026-07-15: the matched four-thread small-allocation
  workload measured `[10.215, 11.440, 12.975] us` with the profiler disabled and
  `[2.2952, 2.3488, 2.4254] ms` with leak detection enabled. This establishes an
  empirical overhead baseline; it does not attribute the delta to one shared
  operation.
- Matched post-change A/B: disabled `[9.9740, 10.668, 11.417] us` with no
  significant change (`p = 0.67`); leak detector `[2.2389, 2.2623, 2.2816] ms`,
  `-4.7386%` median change with `p = 0.00`. The occupancy-flag change is retained;
  pointer mixing remains unmodified because this increment did not measure a
  routing shortfall.
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-prof/src/sampler/`,
  ADR 0004, and PM artifacts; first sampler increment merged in PR #17 at
  `1c91baf`. Hashing and stack interning now live in canonical leaves with
  colocated value-semantic tests; public contracts are unchanged.
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-prof/src/sampler/capture.rs`
  and `sampler/mod.rs`; capture increment merged in PR #18 at `3a6b643`.
  Bounded frame capture and sampling-interval generation now have one canonical
  leaf, with sampling semantics and public contracts unchanged.
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-prof/src/sampler/store.rs`
  and `sampler/mod.rs`; store increment merged in PR #19 at `a281082`.
  Active-sample storage, accounting, lifecycle, and detached snapshots now have
  one canonical leaf with public contracts unchanged.
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-prof/src/sampler/sampling.rs`
  and `sampler/mod.rs`; sampling increment merged in PR #20 at `7046976`.
  Profiler reset and allocation/free sampling orchestration now have one
  canonical leaf with crate-visible hook contracts unchanged.
- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-prof/src/sampler/report.rs`
  and `sampler/mod.rs`; final ADR 0004 sampler extraction is complete on
  `codex/mnemosyne-split-sampler-report`. Symbol resolution, folded-profile
  aggregation, leak-report formatting, and file output now have one canonical
  leaf. Acceptance evidence: warning-denied Clippy, focused nextest 15/15,
  doctests, rustdoc, and final Criterion row `[1.0638, 1.0669, 1.0723] us`
  with a `-1.0618%` point change reported within the noise threshold. No
  performance improvement is claimed.

Filed from the 2026-06-27 deep contention/memory audit (read-only fan-out over
arena/local/core/heap/backend). Ranked by value; each carries a testable
acceptance criterion and named blocker so it is Definition-of-Ready.

Added from the 2026-06-27 deep audit of the under-examined crates
(`mnemosyne-prof`, `mnemosyne-c-shim`, `mnemosyne-heap` containers):

- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-arena/src/segment/{mod.rs,alloc.rs,alignment.rs}` and `crates/mnemosyne-benchmarks/benches/allocator/{mod.rs,allocation.rs,failure.rs,platform.rs,registration.rs,cross_thread.rs,latency.rs,realloc.rs,segment.rs,throughput.rs,workers.rs}`; last-update=2026-07-24. Replaced generic `utils.rs`/`helpers.rs` leaves with named vertical modules whose files each own one concern. Acceptance met: no touched production or benchmark module is named `utils` or `helpers`; alignment, benchmark registration, benchmark failure/policy, and allocation operations retain one canonical implementation; focused package checks, benchmark-target compilation, arena nextest 43/43, benchmark nextest 20/20, all-target/all-feature Clippy, doctests, rustdoc, and baseline-revision semver checks pass; no runtime or benchmark result claim is made.

- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-local/src/local_alloc/page.rs`, new `page/` leaves, callers, and matching PM entries; last-update=2026-07-26. Split the 546-line page module into named leaves for pointer provenance, page-local allocation/reclaim, branded intrusive-list primitives, and allocator list transitions. Acceptance met: the manifest contains no implementation, each leaf owns one concern without duplicated logic or compatibility aliases, all existing callers retain value-semantic behavior under 169/169 nextest tests, the touched crate is warning-clean, and no performance gain is claimed.

- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-core/src/{size_class.rs,types/page/init.rs}` and `crates/mnemosyne-local/src/local_alloc/routing.rs`, with matching PM entries; last-update=2026-07-26. Replaced the three production `unreachable_unchecked` exits with safe checked boundaries: a const lookup-table guard, the canonical corruption abort for an exhausted page, and null propagation for an unavailable current segment. Acceptance met: production source scan is clear, valid allocation behavior passes 187/187 focused all-feature nextest tests, formatting/check/Clippy/doctest/rustdoc gates pass, and no performance gain is claimed.

- [x] [patch] **Shared helper for the cached-pointer TLS fast path — decided
  against, trigger re-checked.** The check-cell-or-OS-slot, reconstitute, guard,
  run shape repeats between `with_allocator` and `with_allocator_unguarded`
  across `CachedCellTls`, `NativeOsTls` and `AsmTls`, but the providers differ
  in slot mechanism and in guard-versus-unguard semantics, so a shared helper
  would obscure the hot path for little gain. The recorded re-open trigger was a
  fourth caching provider appearing.
  Re-checked rather than assumed: the crate has five providers, but `NightlyTls`
  and `StandardTls` reach their slot directly and cache no pointer, so there are
  still three caching providers. The trigger has not fired and the decision
  stands; closed as decided rather than left open indefinitely.
  Checking it turned up something else — `NightlyTls` was still calling the
  `&self` forms of `with_allocator`/`with_allocator_unguarded` that MN-440
  replaced, so `mnemosyne-local` did not compile under `nightly_tls_active`.
  Fixed in the same change and verified with
  `RUSTFLAGS="--cfg nightly_tls_active" cargo +nightly check`.

- [ ] [perf-experiment] status=blocked owner=codex scope=`crates/mnemosyne-arena/src/segment/pool/{cache_aligned.rs,tagged_stack.rs,list.rs}`, `crates/mnemosyne-benchmarks/benches/segment_lock.rs`, and matching PM entries; last-update 2026-07-23. Benchmark whether combining the reclamation-safe pool stack's
  `head` + `count` onto ONE cache line beats the current per-atomic isolation.
  Every push/pop touches both atomics, so a single 64-byte line would touch one
  line per op (not two) and halve the bucket BSS (64 B vs 128 B/bucket; ~96 KiB
  across 256 buckets x 6 backends). The current `TaggedSegmentStack` keeps them
  separate, matching the peer's deliberate "per-atomic cache-line isolation"
  choice — overturning it needs a clean Criterion A/B on the warm pool rows
  (huge cycle/dealloc, cross-thread handoff, segment cache eviction, burst
  retention), not just the threshold gate. Acceptance: A/B shows neutral-or-
  better on those rows -> combine + keep the BSS win; else keep separate +
  document the measured reason. Blocker: this host cannot link the Criterion
  executable: GNU UCRT linking fails against the Rust MSVCRT libraries, the
  MSVC toolchain lacks its C/C++ include environment, and the alternate mingw64
  linker exits 8. Re-open on a Windows host with one coherent native toolchain
  and a quiet benchmark run. Rename `CacheAlignedAtomicPtr` (it is a tagged
  head, not a bare ptr) when this lands.

Filed from the 2026-07-01 four-agent audit cycle (high-severity findings were
fixed in the same cycle — see `## Completed`; these are the deferred
remainder, each Definition-of-Ready):

- [x] [arch] status=done owner=codex scope=`crates/mnemosyne-core/src/{sync.rs,types/{block.rs,page/reclaim.rs,segment.rs}}`, `crates/mnemosyne-local/src/{alloc.rs,free.rs,per_cpu.rs,realloc.rs,local_alloc/,lib.rs,tls_slot.rs,Cargo.toml}`, `crates/mnemosyne-heap/src/raw_heap.rs`, `Cargo.lock`, and matching tests/ADR/PM entries; last-update=2026-07-23. AR-1: Implemented ADR 0001 Option C. The selector macro now provides independent standard and encrypted TLS allocator slots, and policy const propagation selects the slot without a standard-path segment-header load. Owner free, realloc, atomic cross-thread publication, branded raw-heap free, and per-CPU cache publication use the owning segment's `free_list_encrypted` mode as the encoding SSOT. Added mixed-policy alloc/free/realloc value-semantic regressions with pointer-reuse assertions and distinct-slot assertions. The required public seam change is classified as `mnemosyne-local` 0.3.0. Evidence: focused touched-file rustfmt check; warning-denied Clippy; `cargo test --doc --manifest-path crates/mnemosyne-local/Cargo.toml --locked --all-features`; `cargo doc --manifest-path crates/mnemosyne-local/Cargo.toml --locked --all-features --no-deps`; `cargo nextest run --manifest-path crates/mnemosyne-local/Cargo.toml --locked --no-fail-fast` 63/63; release nextest 63/63; branded heap nextest 51/51. No benchmark speedup claim; codegen/criterion proof remains separate performance work.
- [ ] [patch] AR-4: benchmark gate statistics are too weak for the 1.05
  threshold: `sample_size(10)` / 500 ms measurement yields CI widths the
  variance report itself flags at 15-25%. Fix: raise measurement time/samples
  for the gated rows (or gate on median with CI overlap), keep quick settings
  for exploratory rows. Blocker: quiet machine for re-baselining — re-verified
  2026-08-19 rather than taken on trust, and still live: the host was running
  six concurrent compiler processes with several stack repos under active peer
  edits. Memory-bound criterion rows are exactly what that contaminates, so a
  baseline taken here would encode the contention rather than the allocator.
  Not deferred for want of effort: raising the sample counts is a two-line
  change, but doing it without re-baselining invalidates the recorded baseline,
  and re-baselining is the part that needs the quiet host. Both halves have to
  land together.
  Acceptance: gated-row CI half-width < the 5% threshold on the recorded
  baseline.
## Completed

- 2026-07-15 [patch] Profiler contention audit: code review identified the
  global `ACTIVE_SAMPLES_COUNT.fetch_add/sub` and pointer-modulo shard routing
  as the candidate shared lines, while the current Criterion measurement is
  single-thread only. Recorded the leak-detector small-cycle median and
  confidence interval above; the Windows flamegraph attempt returned
  `NotAnAdmin`. No hot-path mutation is accepted until a multi-thread A/B is
  available.

- 2026-07-06 AR-8 [minor]: `mnemosyne-prof` stack interning now routes
  captured stacks across 64 cache-line-aligned shards by stack hash, encodes
  the shard in `StackId`, recycles ids per shard, and constructs first-seen
  `Arc<[usize]>` values outside the shard lock with a race-safe recheck before
  insertion. Added focused shard distribution, id-encoding, same-shard reuse,
  and concurrent distinct-shard interning tests. Added the real
  leak-detector-on Mnemosyne alloc/free Criterion group and summary filter.
  Current measured medians: small/32 `1.1940 us`, medium/1024 `1.1215 us`,
  large/8192 `1.1543 us` (10 samples, 500 ms measurement, Windows host).
  Evidence tier: value-semantic tests plus empirical Criterion measurement.

- 2026-07-02 consolidation cycle 3 (branch fix/audit-2026-07-soundness-perf,
  five atomic commits; detail in CHANGELOG.md and checklist.md). Closed:
  - **AR-1 step 1** [arch, interim]: ADR 0001's debug tripwire landed —
    `Segment::cookie_for::<P>` (the single encode/decode chokepoint) debug-
    asserts the policy's `ENABLE_FREE_LIST_ENCRYPTION` matches the segment's
    recorded mode; three latently-unsound integration tests restructured, a
    `should_panic` pin added, the contract documented on `thread_alloc`.
    The full type-level fix landed: ADR 0001 is Accepted, and the selector
    keys the TLS slot by encryption class — `EncryptedSelectedTls` alongside
    `SelectedTls`, reached through `with_allocator_for_policy`. The "pending
    ADR sign-off" note was stale; verified against the selector macro rather
    than the record.
  - **AR-7** [minor→major]: edition 2024 / resolver 3 across all 11 crates;
    `rust-version = 1.87` (clippy MSRV proved 1.85 dishonest —
    const `is_multiple_of`); 30 `unsafe extern`, 19 `#[unsafe(no_mangle)]`,
    granular `unsafe_op_in_unsafe_fn` blocks, style-2024 reformat. Breaking:
    consumers need Rust 1.87+.
  - **AR-9** [minor]: fuzz `c_shim_api` op-sequence mode (8-slot table,
    seeded write/verify oracles for adjacent-block clobber + realloc chains,
    bounded, leak-free on Drop; 9 smoke tests). libFuzzer run remains
    environment-blocked (g++ C++ runtime); `--lib` path is the evidence tier.
  - **AR-13** [patch]: one authoritative `mnemosyne-build-util` nightly probe;
    all THREE build scripts (prof, benchmarks, local) are thin callers. Also
    fixed a pre-existing latent `nightly_tls` E0432 in `mnemosyne-prof`
    (unconditional import of a `#[cfg(not(nightly_tls_active))]` item), masked
    on this host by the PATH-shadowed nightly rustc; verified by forcing
    `RUSTC` at the real nightly binary.

- 2026-07-02 consolidation cycle 2 (branch fix/audit-2026-07-soundness-perf,
  five atomic refactor commits; detail in CHANGELOG.md and checklist.md).
  Closed deferred items:
  - **AR-3** [patch]: cross-thread reclaim count moved to a per-`ThreadAllocator`
    field folded on `Drop`; the global `fetch_add` is off the reclaim hot path.
    First acceptance clause (no global RMW on the reclaim path) met and
    regression-tested for exact count; the "cross-thread benchmark rows
    neutral-or-better" clause folds into AR-4's re-baseline (a quiet machine),
    tracked there.
  - **AR-5** [patch]: benchmark bodies deduplicated to one generic
    `bench_iter_case`/`bench_batched_case<A>`, one `snmalloc_skips` predicate,
    one `GATE_ROWS` SSOT threshold table (row names unchanged; measured regions
    byte-identical). Follow-up: the nightly-rustc probe is still duplicated
    across `mnemosyne-prof/build.rs` and `mnemosyne-benchmarks/build.rs` — a
    shared build-util/xtask is the fix (filed as AR-13 below).
  - **AR-6** [patch]: local/core SSOT batch — shared `commit_in_place_free` +
    `do_local_free_internal` delegation, `is_sole_active_page`,
    `move_page_between_lists_branded`, `round_up_size` routing,
    `current_thread_id`, `detach_and_release_segment`, `get_next/set_next`
    forwarding, `abort_on_corruption` module, `parent_segment`/`cookie_for`/
    `locate_segment`, `recycle_sweeps` wired, `thread_realloc` branch flatten,
    `cfg(test)` `ThreadAllocator::alloc`, `PER_CPU_CACHE` single-implementor
    invariant documented, `core/types/page.rs` split into leaf modules,
    `kernel_budget` doc fix. `local/local_alloc/page.rs` left un-split
    (≤500 after consolidation, cohesive).
  - **AR-10** [patch]: FOLD `SecurePolicy`/`HardenedPolicy` into
    `mnemosyne-core::policy` (SSOT; core is dep-free); `mnemosyne-hardened`
    retained as a thin real re-export because external (gaia, kwavers) and
    internal manifests reference the crate name.
  - **AR-11** [minor]: `HasSegmentPool` → one required `pools()` with default
    accessors; six per-backend blocks collapse to `BackendPools::new()` +
    one-line impls (−77 lines); `MockBackend` fixtures migrated as the
    consumer half.
  - **AR-12** [patch]: `HandoffBuffer` `unsafe impl Sync` SAFETY comment added.

- 2026-07-01 audit cycle (branch fix/audit-2026-07-soundness-perf, eleven
  atomic commits; per-item detail in checklist.md and CHANGELOG.md):
  [patch] orphan-adoption key preservation + policy-compatibility gate with
  differentially-verified regression tests; [major] `BrandedCell` invariance
  + `unsafe BrandedBlock::cast` (both were safe-code UB); [patch]
  `TaggedSegmentStack::pop` Acquire failure ordering; [minor] huge-pool fit
  cap / derived bucket count / splice restore + huge-pool stats; [minor] CUDA
  module split with init-race atomics, probe-window VEH (silent
  ExitProcess(0) masking removed), full-scan unregister (device-allocation
  leak), test-runner detection deleted; [patch] prof hasher mixing, disabled-
  state sample drain, active-flag serialization, inverted leak flag; [patch]
  decay shutdown lost-wakeup handshake + dead DefaultBackend sweep; [patch]
  c-shim dump_leaks saturation; workspace profiles + committed nextest
  budget; no-op marker features and dead `SpinLock` removed.

- [patch] Repair `mnemosyne-arena` tagged-stack construction for Atlas
  consumers and make huge-pool rejected-chain restoration use the production
  `TaggedSegmentStack::push_chain` batch CAS path. `CacheAlignedAtomicPtr::new`
  is again the no-argument empty-head constructor, and `restore_rejected`
  computes tail/length once before pushing the private chain back as one batch.
  Verification: arena fmt/check/clippy plus downstream Kwavers FWI nextest
  59/59.

- [patch] Add `fuzz/c_shim_api` cargo-fuzz coverage for the
  `mnemosyne-c-shim` ABI. The target accepts arbitrary `(op, size, nmemb,
  alignment)` bytes, shapes them into resource-bounded hostile cases
  (zero-size, invalid alignment, over-`SEGMENT_SIZE` alignment, overflow, exact
  segment edge, and small writable requests), and routes every case through the
  real exported C ABI functions. Assertions pin null-or-valid allocation
  results, alignment, usable-size lower bounds, zeroed calloc prefixes, and
  initialized realloc preservation. The executor also builds as a normal
  no-libFuzzer library for local smoke tests. Local `cargo fuzz run` execution
  is blocked on this Windows install because GNU lacks sanitizer coverage
  support for the target and the MSVC SDK `kernel32.lib` is not installed.

- [patch] Document the `mnemosyne-c-shim` alignment ceiling. The `align <=
  SEGMENT_SIZE` (2 MiB) bound enforced upstream is now stated in the
  `aligned_alloc`/`posix_memalign` rustdoc and `include/mnemosyne.h`: an
  over-large `alignment` yields `NULL` (aligned_alloc) / `ENOMEM`
  (posix_memalign, with `*memptr` untouched), so callers can distinguish it from
  OOM. Doc-only; `cargo doc` clean. The behavior was already covered by the
  adversarial tests added previously; this closes the documentation half of that
  residual (the `cargo-fuzz` infra half remains filed). Also independently
  verified the peer's freshly-merged `shrink_to_len` + NUMA-steal SSOT
  (`numa_bucket.rs` `steal_from`) consolidations: full gate green (256 workspace
  tests, fmt, clippy `-D warnings`), both sound.

- [patch] Consolidate `BrandedVec` shrinking into one `shrink_to_len` helper.
  `shrink_to_fit` and `into_boxed_slice` now share the free-empty/realloc-to-len
  mechanics while `into_boxed_slice` keeps ownership-transfer-specific slice
  construction in place. The residual `extend_trusted` fast path remains
  unmeasured and unfiled until benchmark evidence shows the repeated capacity
  check matters.

- [patch] Consolidate wrap-around NUMA bucket stealing in
  `segment/pool/numa_bucket.rs`. `huge_pool.rs` and `segment_pool.rs` now share
  one `NUMA_BUCKETS` constant, one Themis-backed bucket-index conversion, and
  one generic `steal_from(start, pop_fn)` traversal, leaving each caller to own
  only its pool-specific pop operation. Direct tests pin wrap order and
  early-hit behavior; arena package gates are green.

- [patch] Consolidate the lock-free pool CAS loop into a `TaggedSegmentStack`
  SSOT (`segment/pool/tagged_stack.rs`) and harden it with direct tests. Both
  `NodeHugeBucket` and `NodeSegmentPool` open-coded the identical ABA-immune
  tagged-pointer push/pop/`take_all` CAS loops over `CacheAlignedAtomicPtr`;
  they now embed one `TaggedSegmentStack` (head + retained count) and layer only
  their own cap/telemetry on top, so the ordering + ABA-tag discipline lives in
  exactly one place (SSOT for the safety-critical contention-free path). Because
  the new struct holds only atomics, the FOUR hand-written `unsafe impl
  Send/Sync` (2 per pool) are deleted in favor of compiler-derived
  `Send`/`Sync` — a real reduction of the unsafe surface. Added 3 direct tests
  (LIFO + count, `take_all` chain/count, and a 4-thread×20k-iter conservation
  stress proving the ABA-tag loses no segment), complementing the existing
  pool-level conservation integration tests. Verification: fmt, clippy
  `-D warnings`, 38 arena tests, 254 workspace tests, arena doctests, `cargo doc`
  clean, and `benchmark_summary --enforce-thresholds` (all 12 gated rows within
  threshold). The change is codegen-neutral (the `#[inline]` methods inline into
  the same call sites), so it is a maintainability/safety/test consolidation —
  not a perf change; the head/count cache-line layout is unchanged (a combine is
  filed as a benchmark experiment above).

- [patch] Harden the `mnemosyne-c-shim` C ABI surface with adversarial
  hostile-input tests (the repo mandates panic-free, UB-free, no-unbounded-alloc
  handling of every FFI input). Added 10 tests pinning the boundary contracts the
  happy-path suite omitted: `aligned_alloc` zero/non-power-of-two/over-2-MiB-
  alignment all return null without UB; `aligned_alloc(align, 0)` is null-or-
  freeable; `realloc` shrink preserves `min(old_usable, new)` bytes;
  `posix_memalign` null-memptr/non-pow2 → `EINVAL` (memptr untouched), unsupportable
  alignment → `ENOMEM` (untouched, no UB); `malloc(usize::MAX/isize::MAX+1)` → null;
  `calloc` overflow pairs → null; and a deterministic `(size, alignment)`-grid
  sweep asserting every result is null-or-(aligned+writable+freeable). All pass —
  the boundary is verified sound (no bug found), and the suite is now a regression
  guard. Verification: fmt, clippy `-D warnings`, 23 c-shim tests, 251 workspace
  tests, `cargo doc` clean. Corrected a false prior audit claim in the process
  (`posix_memalign` ENOMEM-for-too-large-alignment is POSIX-correct, not a bug).

- [patch] Consolidate the `BrandedVec` grow mechanics into one `grow_to(new_cap)`
  SSOT (DRY). `push` and `reserve` each open-coded the identical
  `Layout::array → alloc-when-empty / realloc-otherwise → update ptr/cap`
  sequence (~15 lines x2); now both call the single `grow_to` helper and keep
  only their own capacity *policy* (push: initial-4 then ×2; reserve:
  `max(cap*2, needed)`). Correction to the filing: the earlier audit's claim of
  "divergent ×4 vs ×2 growth policies" was wrong — both already used `×2`; the
  `4` in `push` is the initial capacity, and `reserve` sizing to exact `needed`
  is correct, so there was no behavioral bug, only the mechanics duplication. The
  change is behavior-preserving, verified by the existing growth tests that pin
  `capacity()==4` after the first push and reserve sizing (`traits.rs`/`vec.rs`).
  Net subtractive (removed two now-dead imports in `ops.rs`). Verification: fmt,
  clippy `-D warnings`, 51 heap tests, 239 workspace tests, 8 heap doctests,
  `cargo doc` clean.

- [patch] Reduce `mnemosyne-prof` leak/dump memory pressure and contention.
  Live samples now store fixed-width `StackId` handles instead of per-allocation
  `Box<[usize]>` stacks; a refcounted `StackInterner` stores one `Arc<[usize]>`
  per distinct live call stack, increments the refcount on repeats, removes the
  entry on the last free, and recycles id slots. Stack capture uses a fixed
  stack buffer, so repeat call sites do not allocate a boxed frame array.
  `dump_profile` and `dump_leaks` clone active samples into an `ActiveSample`
  snapshot while holding each shard mutex, then release the lock before
  symbolication and file writes. The duplicated nightly/stable TLS sample-insert
  body now routes through `maybe_record_sample`, and pointer-to-shard routing is
  centralized in `sample_shard`. Verification: fmt, stable and nightly-TLS
  checks, clippy `-D warnings`, 7 prof nextest tests including
  `stack_interner_reuses_ids_and_releases_last_reference` and
  `active_sample_snapshot_is_detached_from_live_shards`, prof doctests, and
  `cargo doc`.

- [patch] Close the `// SAFETY:` discipline gap across the **`mnemosyne-prof`**
  crate (25 sites: `tls.rs` 14, `lib.rs` 10, `sampler.rs` 1). The fragile sites
  are now grounded: the TEB inline-`asm!` reads/writes state the Windows x86-64
  TEB layout they rely on (`gs` = TEB base; `gs:[0x1480 + i*8]` = `TlsSlots[64]`;
  `gs:[0x30]` = TEB self-pointer; `TEB+0x1780` = `TlsExpansionSlots`), with
  `# Safety` rustdoc on the two `unsafe fn get/set_teb_tls_slot`; the
  `core::mem::transmute(hook_ptr)` sites state the published-fn-pointer invariant
  (`register_*_hook` stored a real `unsafe extern "C" fn` under Release/Acquire);
  and every `&mut *get_profiler_state()` / `#[thread_local] static mut
  THREAD_STATE` access states the thread-local exclusivity + `in_hook`/`enter_hook`
  re-entrancy-guard invariant. The same sprint also fixed the latent
  `nightly_tls_active` `on_alloc` compile break by routing the allocation
  fast-path state check through `tls::should_skip_alloc_fast_path` instead of
  reaching into private TLS state from `lib.rs`. Verification: fmt, clippy `-D
  warnings`, prof nextest, prof doctests, `cargo doc`, and nightly
  `mnemosyne-prof --features nightly_tls` compile check. This completes the
  crate-by-crate SAFETY sweep across arena/local/core/heap/prof.

- [patch] Close the `// SAFETY:` discipline gap across the **`mnemosyne-heap`**
  crate — the crate the prior arena/local/core closures had missed. Every
  `unsafe` block in `raw_heap.rs` (45 sites), `heap.rs`, `brand.rs`,
  `branded_vec.rs`, `branded_vec/{ops,traits}.rs`, and `branded_box.rs` now
  carries a grounded `// SAFETY:` comment, and both bare `unsafe impl Send`
  (`heap.rs` `Heap`, `raw_heap.rs` `RawHeap`) state the brand-token
  thread-confinement invariant (`ThreadLocalToken<'brand>` is `!Send + !Sync`, so
  the heap cannot be *used* on another thread even if moved; the only interior
  state is `UnsafeCell<ThreadAllocator>` reached under that confinement). The
  GhostCell-style `BrandedCell::borrow`/`borrow_mut`/`borrow_mut_{2,3}` sites
  state the token-aliasing invariant; the raw `*_owned_unchecked` paths state the
  mask-recovered-segment and metadata-slot conventions. Comments only — 382
  insertions, 0 deletions, verified no non-comment line added. The audit also
  re-examined the suspected `insert` panic-safety and `extend` partial-state
  concerns and confirmed both sound (memory-safe contract warts, not bugs).
  Verification: fmt, clippy `-D warnings`, 239 workspace tests, 8 heap doctests,
  `cargo doc` clean.

- [patch] Remove the redundant `with_allocator_guard` TLS entry point (DRY/SSOT).
  It was an exact, zero-caller alias of `with_allocator` (which already arms the
  re-entrancy guard) propagated through two public traits (`TlsProvider`,
  `LocalAllocatorSelector`) — and implemented inconsistently: `native.rs`
  delegated to `with_allocator` while `stable.rs` carried full *duplicated* copies
  of the unsafe `&mut *(ptr as *mut ThreadAllocator)` cached-pointer reconstitution.
  Deleted the method from both trait definitions, the backend-selector macro arm,
  and all six provider impls, shrinking both the API surface and the unsafe-code
  surface to one guarded entry point. Live hot paths (`with_allocator`,
  `with_allocator_unguarded`) untouched, so hot-path codegen is byte-identical
  (the removed method had no callers and emitted no code). Verification: workspace
  builds with no broken caller (proving it was dead), fmt/clippy `-D warnings`,
  239 workspace tests, doctests, `cargo doc` clean.

- [arch] Close the ABA-immunity gap in the lock-free **segment** cache
  (`NodeSegmentPool`), the complement to the huge-pool tagged fix above. Its
  plain `AtomicPtr` head left single-element `pop` ABA-exposed (a stale
  `head X -> next Y` CAS after X is popped+re-pushed orphans the chain and loses
  segments). New `tests/segment_pool_concurrency.rs` provoked the loss; the
  pre-existing `test_concurrent_aba_safeness` missed it by never asserting
  conservation. Head now uses the tagged `CacheAlignedAtomicPtr` (48-bit addr +
  wrapping tag, mirroring `mnemosyne-core::sync::AtomicFreeList`). Stress test
  passes 15/15 (was non-deterministic loss); 239 workspace tests + threshold
  gate clean. Commit `241b795`.

- [patch] Add opt-in `mnemosyne-local/dealloc-probe` branch-mix counters for
  committed `thread_free` arms, with feature-gated value-semantic coverage that
  layout-proven same-owner small frees record as `InPlaceSmall`.
- [patch] Convert the huge-allocation cache's `NodeHugeBucket` from a
  spinlock-protected intrusive list to a lock-free Treiber stack. Exact-bucket
  pops still find a fitting segment behind undersized heads by restoring
  temporarily rejected segments, and shared cache-line atomic wrappers now live
  in `segment/pool/cache_aligned.rs`. The head carries a 64-bit tagged-pointer
  mutation counter to prevent stale-head ABA under concurrent pop/push stress.
- [patch] Resolve the `NodeHugeBucket` alignment tradeoff by replacing
  whole-struct `#[repr(align(64))]` with per-atomic cache-line isolation for the
  contended `head` and `count` fields.
- [patch] Expand the benchmark-summary threshold gate to the selected realloc
  latency rows and refresh `allocator_baseline_excerpt.csv` so enforcement now
  compares twelve selected rows.
- [patch] Clean backend/arena/tiered-heap rustdoc links and evidence wording so
  `cargo doc --workspace --all-features --no-deps` is warning-clean.
- [patch] Continue unsafe-discipline closure in `mnemosyne-core` and
  `mnemosyne-local` by documenting the `Segment` `Send`/`Sync`, Windows TEB
  thread-id read, local-free unchecked pointer/cookie updates, and native/ASM
  TLS allocator-pointer dereference invariants.
- [patch] Close the unsafe-discipline `// SAFETY:` gap across `mnemosyne-arena`
  (arena coordination, segment alloc/pools, and the scratch buffer module);
  every `unsafe` block and `unsafe impl Send/Sync` now documents its invariant,
  with two behavior-neutral consolidations (huge-pool purge drain loop, cached
  huge-segment header read) and the vacuous `ScratchPool::capacity` comment
  replaced by the real `!Sync` invariant.
- [patch] Consolidate initialized large/huge allocation fallback branches into
  one allocator helper in `mnemosyne-local::alloc`.
- [patch] Bound per-CPU cache CPU-id refresh retries after failed CAS attempts
  so each allocation/free attempt performs at most one refresh probe.
- [patch] Consolidate page-local free-list pop and lazy bump allocation into a
  single allocator helper used by `thread_alloc` and `ThreadAllocator`
  allocation paths.
- [patch] Route orphan-segment adoption through the guarded segment-aware
  page remote-free reclaim helper, avoiding empty-queue atomic drains while
  preserving adoption ownership and encryption semantics.
- [patch] Move allocation-side remote-free empty-queue guarding into
  `try_reclaim_and_allocate` so active and full page recovery share one
  helper-owned drain path.
- [patch] Centralize allocator sweep remote-free reclamation through a
  segment-aware `Page` helper that skips empty queues before atomic drains.
- [patch] Skip empty page-local remote-free queues during thread-exit
  owned-segment reclamation, avoiding unnecessary atomic drains while
  preserving live-segment orphaning semantics.
- [patch] Skip empty page-local remote-free queues during periodic allocator
  defragmentation sweeps, avoiding unnecessary atomic drains while preserving
  live-allocation accounting.
- [patch] Route allocator segment reclamation sweeps through a segment-aware
  page cross-thread-free reclaim helper, avoiding repeated parent segment and
  page-index derivation where the caller already owns that metadata.
- [patch] Remove benchmark-summary CSV row `Vec<Cow<_>>` collection and
  benchmark-name clone by parsing required summary fields through a lending
  `Cow` iterator.
- [patch] Remove the missing-selected-benchmark `Vec` allocation from
  `benchmark_summary` threshold enforcement.
- [patch] Remove the benchmark-baseline comparison `Vec` allocation and
  benchmark-name clone from `benchmark_summary` by streaming borrowed
  comparison rows.
- [patch] Remove the selected-baseline excerpt `Vec` allocation from
  `benchmark_summary` by streaming selected rows through an iterator writer.
- [patch] Split `mnemosyne-prof` TLS provider and per-thread hook state into a
  dedicated leaf module, leaving public controls and hook entry points in the
  crate root.
- [patch] Split `mnemosyne` global allocator integration tests into
  bounded-context leaf modules while keeping the root as global allocator and
  shared fixture ownership only.
- [patch] Replace duplicate local allocator TLS seed cache branches with the
  Melinoe thread-cached initialization primitive.
- [patch] Split `mnemosyne-heap` unit tests into bounded-context leaf modules
  under `src/tests/`, keeping the root module as shared fixtures only.
- [patch] Remove benchmark-summary CLI argument collection by parsing known
  flags directly from the iterator with value-semantic parser coverage.
- [patch] Refresh `benchmarks/allocator_comparison.md` with current
  `system-jemalloc` Criterion results and close the initial segment-cache
  eviction alert as measured variance after focused rerun plus threshold gate.
- [patch] Split the benchmark-summary binary into dedicated config, CSV,
  Criterion, report, allocator-rendering, metadata, and threshold leaf modules;
  remove tracked scratch artifacts; and harden report writers so missing
  `target/criterion` parents are created before output files are opened.
- [minor] Add `ScratchBank<T, const N>` as the provider-owned fixed scratch-role abstraction for Apollo transform workspaces, keeping role selection const-generic and avoiding repeated per-role `ScratchPool` statics in consumers.
- [patch] Prevent combined usable-size benchmark cross-optimization by consuming the allocated pointer through `black_box` before size query and deallocation, resolving the stale inverted small/medium/large ordering in `usable size latency`.
- [patch] Add layout-proven `GlobalAlloc::dealloc` routing so Rust callers with the original `Layout` monomorphize out the large/huge free classifier for small allocations while preserving the pointer-only `thread_free` classifier for C-style and unknown-layout callers.
- [patch] Outline active-profiler free-size accounting behind a cold helper so disabled profiling leaves the hot free path with only the existing activity guard.
- [patch] Add active `rpmalloc::RpMalloc` benchmark coverage and reduce the `large_8192` deallocation row by stamping owner allocator cache pointers, bypassing the busy-bit write pair for first frees from full pages, and moving full pages back to active pages with one branded list token.
- [patch] Remove duplicate public cold-allocation defrag cadence charging after `ThreadAllocator::alloc_cold`; the cold refill now charges once at the owning allocator boundary.
- [patch] Add GhostCell-style branded page-list mutation tokens for intrusive active/full/empty page lists, keeping page-list splice and push helpers zero-sized and allocator-permission-gated.
- [patch] Add GhostCell-style branded owned-segment mutation tokens for the intrusive owned-segments list and a Miri-only owner-token fallback that avoids unsupported Windows inline assembly.
- [patch] Carry one branded page-list token through empty-page recycling selection and unlink, preserving dirty-segment prioritization while reducing repeated token/unlink setup on `pop_best_empty_page`.
- [patch] Reject page-local pop/bump helper consolidation in `alloc_class`/cold active-head retry because the monomorphized helper perturbed allocation-cycle codegen and exceeded selected cycle thresholds.
- [patch] Reject current-segment minimum-block free shortcut because focused deallocation-only rows regressed despite improving one noisy small-cycle sample.
- [patch] Reject skipping `initialize_free_list` for never-used fresh pages because the refill-row improvement regressed all selected allocation-cycle gates.
- [patch] Reject direct full-page relink and layout-aware small-free experiments after measurement or invariant checks failed to support retaining them.
- [patch] Restore first-class RpMalloc columns in `allocator_comparison.md` generation so rpmalloc benchmark rows are visible in the comparator table.
- [patch] Reject active-page empty-`thread_free` guards after threshold enforcement showed cycle-latency regressions; keep the existing unconditional active-page reclaim path.
- [patch] Make `nightly_tls` compiler-channel-aware so stable all-feature gates use the portable TLS provider and nightly compilers retain the `#[thread_local]` fast path.
- [patch] Make `nightly_tls_active` build-script cfg generation rerun when `RUSTC` changes, preventing stale compiler-channel detection.
- [patch] Maintain an allocator-local owned-segment count so segment reclaim and defragmentation threshold checks no longer rescan the owned list.
- [patch] Split thread-local segment ownership and reclamation into `local_alloc/segment/ownership.rs` and `local_alloc/segment/reclaim.rs`.
- [patch] Remove per-iteration heap allocation from the cross-thread handoff benchmark, run the jemalloc-enabled allocator comparison, and refresh the threshold-gated benchmark baseline after verifying stale cross-thread and saturated-threaded rows against unmodified `HEAD`.
- [patch] Remove non-owner defragmentation accounting from remote-free enqueue, resolving the `cross-thread free handoff/mnemosyne/small_32` regression while preserving owner-side reclamation.
- [patch] Replace threaded allocation-cycle worker `Vec` storage with fixed arrays and regenerate the jemalloc-enabled comparison, resolving the stale `threaded small allocation cycles/mnemosyne` regression row.
- [patch] Align small-allocation `usable_size` page-index derivation with the deallocation classifier and regenerate stable usable-size comparison rows.
- [patch] Move thread-local allocator statistics into a dedicated leaf module and compute diagnostic snapshots from active/full/empty page lists instead of segment-wide page scans.
- [arch] Consolidate the two public heap wrapper surfaces into one scoped `Heap<'brand, P, B>` API backed by the single monomorphized `RawHeap<P, B>` implementation.
- [patch] Supersede the earlier wrapper-column allocator report shape: `MnemosyneHeap` and `BrandedHeap` are no longer classified as allocator comparators, and stale Criterion rows are ignored.
- [patch] Include SnMalloc `huge_2m` benchmark rows in allocator comparisons instead of hard-coded `N/A` omissions.
- [patch] Replace remaining Rustdoc example unwraps with contextual allocation diagnostics.
- [patch] Replace benchmark utility unwraps with explicit full-page handling and contextual layout diagnostics.
- [patch] Replace the local allocator page-saturation test panic with a value assertion carrying segment/page diagnostics.
- [patch] Replace remaining branded vector shrink and extension unwraps with operation-specific diagnostics.
- [patch] Replace bare branded container trait-operation unwraps with operation-specific diagnostics.
- [patch] Replace bare branded-cell test allocation and vector-push unwraps with operation-specific diagnostics.
- [patch] Replace bare branded-vector transition push unwraps with operation-specific diagnostics.
- [patch] Replace bare heap unit-test layout unwraps with a shared contextual layout helper.
- [patch] Replace bare heap integration test layout and worker-join unwraps with contextual diagnostics.
- [patch] Harden global allocator leak-detector integration test with guarded profiler/allocation cleanup and contextual dump diagnostics.
- [patch] Replace bare policy integration test layout/thread-join unwraps with contextual diagnostics.
- [patch] Harden local topology tests with contextual diagnostics and an RAII guard for the global per-CPU cache flag.
- [patch] Replace the remaining bare C-shim leak-report `CString` unwrap with contextual diagnostics.
- [patch] Reconcile `docs/complexity_audit.md` with the current free-list/bump-page allocator after the bitmap free-list experiment was rejected.
- [patch] Replace bare segment-layout unwraps in `mnemosyne-core::types` tests with a single contextual layout helper.
- [patch] Harden `mnemosyne-prof` integration tests with contextual diagnostics and RAII cleanup for global profiler state and live thread allocations.
- [patch] Remove production panic paths from OS TLS key initialization; native TLS failure now falls back to standard thread-local state for allocator and profiler access.
- [patch] Relax profiler OS TLS-key publication to relaxed atomic ordering; the key is an immutable slot index and protects no Rust memory dependency.
- [patch] Harden profiler sample storage against poisoned shard locks and bounded stack-capture overflow while preserving exact retained stack slices.
- [patch] Clean up the clippy-reported nested occupancy-mask branch in `Page::set_alloc_count_for_segment`.
- [patch] Include the `Threaded medium allocation cycles` Criterion group in benchmark-summary extraction and generated allocator comparison reports.
- [patch] Pin benchmark-summary active-group filtering with unit tests so all allocator benchmark groups are retained and exploratory TLS benchmark rows stay out of allocator comparison summaries.
- [patch] Make benchmark threshold enforcement reject incomplete current Criterion data when any selected baseline row is absent.
- [patch] Document the generated variance report and selected-row completeness requirement in the benchmark workflow.
- [patch] Apply the scheduler-aware variance threshold to retained medium-threaded allocation rows.
- [patch] Report `Mnemosyne`, `MnemosyneHeap`, and `BrandedHeap` as distinct allocator comparison rows using exact allocator classification.
- [patch] Convert the remaining `BrandedHeap` benchmark allocation failure from `expect` panic to explicit benchmark failure diagnostics.
- [patch] Consolidate explicit and branded heap mechanics behind a shared monomorphized `RawHeap<P, B>`.
- [patch] Keep `MnemosyneHeap` available from `mnemosyne_heap` while removing it from the top-level `mnemosyne` shell re-export.
- [patch] Keep `RawHeap` large/huge deallocation code in one cold helper shared by explicit and branded free paths.
- [patch] Preserve profiler sample memory efficiency with exact captured stack slices while retaining sharded active-sample maps.
- [patch] Retain the active thread-local segment during local frees so hot allocate/free cycles reuse page free lists instead of scanning and recycling the segment.
- [patch] Replace single-shape allocator benchmarks with Criterion cycle, burst-retention, and threaded comparison groups for Mnemosyne, mimalloc, and snmalloc.
- [patch] Fix Unix backend constant typing so Rustfmt can parse all target modules.
- [patch] Add Mnemosyne backend and arena memory telemetry for mapped bytes, peak mapped bytes, map/unmap calls, retained free segments, and retained free bytes.
- [patch] Bound the global free segment cache to one segment-turnover window and release additional empty segment mappings to the OS.
- [patch] Add cross-thread free handoff benchmarks for Mnemosyne, mimalloc, and snmalloc.
- [patch] Avoid invoking segment-reclaim logic on hot local frees when the page belongs to the current thread-local segment.
- [patch] Add current-thread live allocation, current-thread owned segment, and cross-thread reclaimed block telemetry.
- [patch] Add `memory_report` CSV output for direct Mnemosyne memory telemetry inspection.
- [patch] Replace per-iteration cross-thread benchmark thread creation with persistent bounded-channel handoff workers.
- [patch] Add per-size-class occupancy telemetry for active pages, empty pages, live allocations, and total slots.
- [patch] Replace threaded allocation benchmark thread creation with persistent bounded-channel worker sets.
- [patch] Add deterministic segment-cache eviction benchmark coverage and `memory_report` eviction telemetry.
- [patch] Add arena purge telemetry for purged segments, purge calls, and purged bytes.
- [patch] Add `benchmark_summary` release command that extracts compact Criterion mean/median estimates to CSV.
- [patch] Add `purge_after` memory report scenario proving retained segment cache purge behavior.
- [patch] Add source-controlled selected Mnemosyne benchmark baseline excerpt.
- [patch] Add benchmark baseline metadata documenting platform, toolchain, and benchmark commands.
- [patch] Add current-to-baseline benchmark comparison CSV generation for selected Mnemosyne rows.
- [patch] Restore small-allocation segment pointer scope in `thread_free` after the large-allocation classifier.
- [patch] Add value-semantic tests for benchmark summary CSV parsing and baseline ratio computation.
- [patch] Restore missing assertion delimiter in the memory retention-bound test.
- [patch] Make the page-recycling test assert segment reuse and target size-class metadata instead of global-state-sensitive exact page index.
- [patch] Require explicit `--refresh-baseline` for source-controlled benchmark baseline mutation.
- [patch] Route cross-thread small frees to the owning page queue instead of the owner allocator queue.
- [patch] Remove duplicate segment-address derivation from `thread_free`.
- [patch] Preserve hot local allocation path by reclaiming page-local remote frees only after local free blocks are exhausted.
- [patch] Centralize page-local cross-thread free reclamation in an inlined `Page::reclaim_thread_free` method.
- [patch] Add direct value-semantic coverage for `Page::reclaim_thread_free`.
- [patch] Bind the global allocator and local allocator tests to the zero-sized `StandardPolicy` after policy-generic allocation APIs were introduced.
- [patch] Remove the panic-bearing `align_up` API and keep checked alignment as the single production alignment contract.
- [patch] Make benchmark regression threshold enforcement explicit with `--enforce-thresholds` so quick-mode summaries remain non-gating.
- [patch] Move generated benchmark metadata from `benchmarks/metadata.json` to `target/criterion/benchmark_metadata.json`.
- [patch] Stabilize page-recycling test allocation-count expectations against reusable orphan/global segment state.
- [patch] Gate benchmark metadata path constant out of test builds to keep diagnostics warning-clean.
- [patch] Centralize allocation initialization and free poisoning behind monomorphized `AllocPolicy` helpers.
- [patch] Serialize allocator integration tests that mutate process-wide segment-pool state.
- [patch] Derive hard regression threshold policy from repeated non-quick benchmark samples on the same hardware.
- [patch] Re-benchmark cross-thread 32-byte handoff against mimalloc after page-queue routing.
- [patch] Audit remaining allocator panic sites in tests and benchmark-only utilities.
- [patch] Convert benchmark-only panic assertions in memory_report to explicit Result errors.
- [patch] Replace raw segment owner pointers with a transparent `SegmentOwner` permission token.
- [patch] Remove allocator-level `incoming_free_list` after page-local remote-free routing made it redundant.
- [patch] Add direct test coverage for re-entrant local free fallback through the page-local atomic queue.
- [patch] Complete backend-specific segment-pool typing through `HasSegmentPool` exports and arena call-site bounds.
- [patch] Reject single-TLS local-free rewrite after focused benchmark showed a statistically significant regression.
- [patch] Reject `UnsafeCell` allocator permission split after focused cycle benchmark confirmed hot-path regression.
- [patch] Add a saturated threaded small-allocation benchmark group to isolate allocator throughput from bounded-channel worker coordination overhead.
- [patch] Fix backend-specific thread-local allocator selector generation so each backend receives distinct TLS storage.
- [patch] Run the saturated threaded small-allocation benchmark against Mnemosyne, mimalloc, and snmalloc.
- [patch] Add per-thread page-refill telemetry and defer recycle sweeps until the current segment is exhausted.
- [patch] Reject single-TLS local-free collapse after historical threaded benchmark exceeded the configured threshold.
- [patch] Replace the scheduler-sensitive historical threaded baseline gate with the saturated threaded baseline row.
- [patch] Convert benchmark runner panic assertions and channel unwraps to explicit benchmark failure diagnostics.
- [patch] Add local safety contracts to benchmark unsafe operations and allocator policy byte-initialization helpers.
- [patch] Audit backend-specific CUDA unified-memory tracking for bounded metadata and zero-cost fallback behavior.
- [patch] Synchronize README architecture notes with page-local remote-free routing and CUDA fallback behavior.
- [patch] Audit production unsafe blocks in `mnemosyne-backend` for local safety contracts and ordering minimality.
- [patch] Audit backend allocation failure accounting so telemetry cannot record unmapped bytes before OS release succeeds.
- [patch] Audit arena purge accounting so purged segment counters only count confirmed backend releases.
- [patch] Audit ignored backend release results in large-allocation cleanup paths.
- [patch] Audit large-allocation metadata layout for alignment guarantees and metadata-slot bounds.
- [patch] Audit small-allocation free classification for invalid-alignment and metadata-boundary failure modes.
- [patch] Audit allocator alignment request handling so invalid public `Layout` alignments cannot reach arena alignment math.
- [patch] Audit zero-size allocation behavior for `GlobalAlloc` and direct `thread_alloc` callers.
- [patch] Audit allocation request size bounds against `Layout` maximum and backend mapping arithmetic.
- [patch] Audit duplicated allocation request validation across global, local, and arena entry points.
- [patch] Tighten huge-allocation backend mapping size and pin the memory-efficiency contract with telemetry.
- [patch] Remove dead page back-pointer metadata and keep `Page` within one cache line.
- [patch] Audit generated benchmark artifact freshness and documentation references for the current allocator comparison set.
- [patch] Audit test-only panic diagnostics without reducing assertion strength.
- [patch] Audit production debug assertions for value-semantic invariant messages and zero-cost release behavior.
- [patch] Audit local allocator remote-free reclaim paths for duplicated block-pop logic.
- [patch] Investigate full all-allocator Criterion quick-run timeout while focused gated rows complete.
- [patch] Guard local-free full-page reactivation on confirmed full-list unlink.
- [patch] Audit benchmark baseline metadata after bounded Criterion harness configuration.
- [patch] Refresh source-controlled benchmark baseline excerpt from bounded Criterion harness output.
- [patch] Optimize thread_free segment owner check by introducing get_allocator_ptr to LocalAllocatorSelector.
- [patch] Add jemalloc to allocator benchmark comparator coverage and generated comparison reports.
- [patch] Add opt-in segment tail guards without default benchmark overhead.
- [patch] Extend memory report with page-reset and guard-install telemetry.
- [patch] Force cross-crate inlining for size-class mapping on allocator hot paths.
- [patch] Move secure-policy small-free poisoning after classification so the small page metadata lookup is shared.
- [patch] Reject layout-aware `GlobalAlloc::dealloc` small-free classification after saturated threaded benchmark regression.
- [minor] Add usable-size latency benchmarks for Mnemosyne, mimalloc, snmalloc, and target-gated jemalloc.
- [patch] Optimize `usable_size` small-allocation classification by reading target page metadata before the Page 0 huge-allocation fallback.
- [minor] Override `GlobalAlloc::realloc` with an in-place standard-policy fast path when the new request fits in `usable_size(ptr)`.
- [patch] Preserve secure-policy realloc zero-initialization by forcing replacement allocation on growth.
- [minor] Add realloc latency benchmarks for within-class and cross-class realloc cycles across Mnemosyne, mimalloc, snmalloc, and target-gated jemalloc.
- [minor] Add isolated usable-size query latency benchmarks that separate metadata lookup cost from allocation/deallocation cost.
- [minor] Add allocation-only latency benchmarks with drop-guard cleanup to separate allocation cost from deallocation cost.
- [minor] Add system allocator comparator rows to the allocator benchmark matrix and generated comparison reports.
- [patch] Optimize small-free classification and local-free owner checks to remove duplicate metadata and TLS work from deallocation hot paths.
- [minor] Add deallocation-only latency benchmarks to isolate free-side cost across Mnemosyne, System, mimalloc, snmalloc, and target-gated jemalloc.
- [patch] Remove dead `Page::local_free` state and allocation fast-path branch after verifying all local frees route through `Page::free`.
- [patch] Add small-realloc size-class proof fast path to avoid `usable_size` metadata lookup when the old `Layout` already proves the existing class covers the new request.
- [patch] Add a current-segment marker so same-thread frees on the active segment bypass the allocator-cell mutable borrow when no page-list mutation or segment reclaim is required.
- [minor] Add `LocalAllocatorSelector::with_allocator_guard` so allocation guard setup, allocator access, and guard clear happen inside one selector operation.
- [patch] Replace hot-path size-class arithmetic with a compile-time lookup table generated by `const` evaluation.
- [minor] Replace thread-local allocator `RefCell` access with guarded `UnsafeCell` access under the allocation flag.
- [patch] Add variance-aware benchmark report generation for Criterion mean confidence intervals and unstable-row classification.
- [patch] Centralize huge-allocation suffix sizing in `Segment::huge_mapping_suffix_from` and route `usable_size` plus secure free poisoning through it.
- [patch] Reject precomputed-class allocation dispatch and direct realloc-capacity arithmetic after focused Criterion rows showed threaded and realloc regressions.
- [patch] Reject layout-aware small-deallocation bypass after saturated threaded rows regressed despite isolated deallocation improvement.
- [patch] Document realloc slow-path copy bounds so size-class slack bytes are not propagated as initialized data.
- [patch] Collapse the per-thread allocation guard and allocator cache into one TLS slot, reducing small allocation/free cycle TLS lookups while preserving the re-entrant fallback contract.
- [patch] Reject forced cross-crate inlining of `AtomicFreeList` operations after cross-thread handoff improved but saturated threaded cycles regressed.
- [patch] Reject `thread_local!` const initialization for the allocator slot after it improved non-saturated rows but regressed saturated threaded cycles.
- [patch] Add all-size-class lower-bound coverage for `usable_size` so small allocations can never under-report class capacity.
- [patch] Reject separate owner-token TLS routing after cycle latency and cross-thread handoff regressed.
- [patch] Extract shared monomorphized realloc slow path so both allocator implementations use one copy-length contract.
- [patch] Force inlining of the shared realloc slow-path helper after focused Criterion rows improved both retained realloc latency regressions.
- [patch] Reject the <=128-byte arithmetic realloc capacity shortcut after its absolute point estimate missed the accepted within-class realloc row and polluted allocator-cycle measurements.
- [patch] Reject deferred remote-free telemetry accounting after it failed to improve small cross-thread handoff and regressed medium handoff plus historical threaded allocation cycles.
- [patch] Reject forced inlining of `Page::reclaim_thread_free` after refreshed historical threaded allocation cycles regressed despite one saturated sample improving.
- [patch] Reject forced inlining of exported `usable_size` after combined usable-size and allocator-cycle rows regressed.
- [patch] Reject a Layout-proven small-allocation entry split after it improved allocation-only latency but widened the retained small cycle and threaded-small gaps.
- [patch] Serialize backend telemetry tests that mutate process-wide mapping counters so workspace tests are deterministic.
- [patch] Reject compact `Page` counter layouts after 48-byte metadata experiments regressed saturated threaded and usable-size rows.
- [patch] Centralize the 16-byte small-block floor as `MIN_BLOCK_SIZE` and remove stale compact-counter invariants.
- [patch] Reject removing the `MAX_ALLOC_SIZE` check from the Layout-validated allocation predicate after focused Criterion rows improved cycle/usable means but regressed allocation-only and historical threaded small rows.
- [patch] Reject Bitmap Free Lists for classes 0, 1, and 2 after Criterion small allocation cycles, realloc, and threaded allocation benchmarks regressed.
- [patch] Reject Bounded Retention of Huge Mappings and per-CPU cache optimizations after allocator burst retention and threaded cycles regressed.
- [patch] Make branded heap containers allocation-free for zero-sized types and reject overflowing `BrandedVec` capacity growth before layout construction.
- [patch] Make primitive branded heap initialization/free ZST-aware so `alloc_init::<T>`, `free`, and `free_uninit` share the same allocation-free zero-sized-type contract as the safe containers.
- [patch] Make primitive branded heap realloc ZST-aware so zero-sized source permissions never route dangling pointers through usable-size, byte-copy, or raw-free allocator logic.
- [patch] Preserve the `len <= capacity` vector invariant for `BrandedVec::new::<ZST>` by installing the allocation-free sentinel capacity at construction.
- [patch] Make `BrandedVec::into_boxed_slice` attempt an explicit shrink instead of relying on same-pointer shrink realloc, while preserving the original buffer if replacement allocation fails.
- [patch] Wire secure and hardened allocation policies to seeded page free-list randomization while preserving the standard policy lazy bump path.
- [patch] Route heap-local small allocation through `ThreadAllocator::alloc_class` as the single active-page pop/bump implementation.
- [patch] Restore same-pointer shrink behavior in `thread_realloc` through the existing small-realloc size-class proof.
- [patch] Avoid allocate-copy-free churn for standard-policy large/huge half-shrink reallocs and bound all replacement realloc copies to `min(layout.size(), new_size)`.
- [patch] Reduce leak/profiling stack-sample memory by capturing into fixed stack storage and retaining only exact-length boxed stack slices.
- [arch] Split `mnemosyne-core` allocator types, `mnemosyne-arena` segment pools/tests, `mnemosyne-local` top-level allocation/free/realloc/TLS/options helpers, `mnemosyne-prof` sampling/reporting, `mnemosyne-c-shim` tests, and `BrandedVec` operations/trait impls into cohesive leaf modules while preserving public re-exports and monomorphized APIs.
- [patch] Stabilize memory-stat tests after leak-detector thread-exit orphan adoption by asserting allocation-count deltas instead of a false absolute baseline.
- [arch] Split heap, branded-container, local-allocation page/routing/segment, and global allocator test surfaces into cohesive modules while preserving monomorphized hot-path APIs and public re-exports.
- [patch] Remove stale imports from split local allocator modules so warning output stays clean and real allocator regressions remain visible.
- [patch] Retain `threaded medium allocation cycles/` in generated benchmark summaries and comparison reports.
- [patch] Use `benchmark_variance.csv` to retest remaining within-class realloc and historical threaded-row optimizations before accepting allocator changes.
- [patch] Investigate cross-thread handoff batching or owner-token routing without increasing saturated threaded cycles.
- [patch] Investigate mimalloc's remaining within-class realloc, historical threaded-row, saturated threaded-row, cross-thread handoff, and usable-size combined-cycle advantages after the unified TLS slot narrowed saturated threaded disparity.
- [patch] Run the jemalloc comparator leg on a target where `tikv-jemallocator` links and refresh comparison rows.
- [patch] Fix decay engine thread-spawning shadowing bug and add `decay_purger_reaches_steady_state` integration test.
- [patch] Expose `get_options` and `configure` in the top-level `mnemosyne` crate and verify via programmatic configuration tests.
- [patch] Add `multi_heap_isolates_allocation_streams` and `multi_heap_release_does_not_touch_other_heaps` integration tests.
- [patch] Consolidate public allocator periodic-defragmentation accounting into a shared `ThreadAllocator::record_defrag_operation` cold-sweep boundary.
- [patch] Reject extending the shared defrag-accounting helper to `RawHeap` after explicit/branded cycle rows regressed; heap-local hot paths retain their inline accounting shape.
- [patch] Split page allocation-counter updates into monomorphized increment/decrement helpers and pass known page indices through free paths so occupancy-mask maintenance avoids redundant page-index recovery.
- [patch] Route same-owner small cross-class realloc through the raw allocator pointer with an explicit re-entrancy flag, avoiding the closure guard overhead while preserving local free semantics.
- [patch] Bound periodic defragmentation owned-segment counting by the reclaim threshold instead of traversing the whole owned list once four segments are known.
- [patch] Iterate segment reclaim and defragmentation over the occupied-page bitmask instead of scanning every page in mostly empty segments.
- [patch] Relax hot TLS-key reads from acquire to relaxed ordering because the key is an immutable OS slot index, not a protected allocator data dependency.
- [patch] Store each page's segment-local index in metadata and route `page_start` plus occupancy-mask transitions through that stored index, avoiding repeated page-address subtraction/division while keeping `Page` within one cache line.
- [patch] Use page allocation-counter increment helpers on local and heap allocation hot paths so occupancy-mask maintenance does not reload and compare an already-derived target count.
- [patch] Refresh allocator comparison rows after stored page-index routing; current saturated threaded small cycles measure Mnemosyne `66.851 us` versus mimalloc `70.088 us`.
- [patch] Charge periodic defragmentation cadence only when local free transitions actually make a page empty, removing sweep accounting from full-page-to-active transitions and closing `allocator deallocation latency/large_8192` versus jemalloc.
- [patch] Keep current-segment occupancy-mask bits conservative across local frees, removing repeated mask clear/set traffic from hot small alloc/free reuse while preserving exact `alloc_count` authority.
- [patch] Derive `usable_size` page indices from the already-computed segment base, removing the shifted-mask index path and refreshing the small usable-size comparator row.
- [patch] Reject the `MAX_SMALL_ALLOC_SIZE` size-class boundary shortcut after the benchmark-summary threshold gate still reported `allocator cycle latency/small_32` above the retained 1.05 ratio despite large-cycle improvement.
- [patch] Replace runtime size-class leading-zero arithmetic with a compile-time-generated `u8` table covering every small allocation size, reducing allocator cycle latency without adding type-specific APIs.
- [patch] Update `melinoe` to the latest `main` commit resolved by Cargo (`85d498bb`, crate version `0.5.0`) and verify `mnemosyne-heap` against the current branded-token API.
- [patch] Remove per-row `Vec`, owned key, formatted-cell, and allocator-name lowercase allocations from `benchmark_summary` allocator comparison generation by splitting benchmark names with borrowed `&str` slices, keeping comparison keys borrowed, streaming cells through `Display`, and classifying allocators case-insensitively without allocation.
- [patch] Remove profiler dump snapshot clones and intermediate symbol vectors by processing active sample maps under shard locks, borrowing exact boxed stack slices, streaming leak samples directly to the report file, and using scoped `Path::to_string_lossy` `Cow` values only at the output boundary.
- [minor] Make the top-level `mnemosyne` branded heap re-export an additive default feature and build allocator benchmarks with `default-features = false`, keeping the default public API unchanged while isolating global allocator latency runs from branded-heap dependency code layout.

## Open

- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-benchmarks/benches/segment_lock.rs`, `crates/mnemosyne-benchmarks/Cargo.toml`, and contention PM entries; last-update=2026-07-15. Isolate `CacheAlignedSegmentLock` uncontended and contended cost using the canonical source implementation and a persistent bounded worker harness. Acceptance: compare the same lock-only rows at the merged and pre-lock provider states, then either retain the existing spin/yield policy or implement a measured upstream adjustment with value-semantic concurrency gates.

  The source-included lock-only Criterion A/B uses a zero-sized unlocked
  reference control and the same persistent four-worker harness. The reference
  rows are `27.859 ps` `[27.669, 28.102] ps` uncontended and `1.5305 us`
  `[1.4876, 1.5834] us` through the worker harness. The lifetime-lock rows are
  `4.5471 ns` `[4.5396, 4.5583] ns` and `201.73 us`
  `[184.35, 222.88] us`. The derived deltas are approximately `4.52 ns` per
  uncontended acquisition and `50.0 ns` per contended acquisition after
  subtracting the shared worker baseline. The existing 64-spin/yield policy is
  retained; no production synchronization mutation is justified.

- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-arena/src/segment/pool/{tagged_stack,cache_aligned}.rs`, allocator benchmarks, and contention PM entries; last-update=2026-07-15. Measure the per-stack lifetime-lock contention introduced by PR #9 against the pre-lock parent under matched Criterion workloads. Acceptance: capture median and confidence interval for segment-cache eviction and threaded/cross-thread rows, then either retain the lock with evidence or implement a correctness-preserving upstream optimization with value-semantic and concurrency gates.

  The merged lock-bearing provider (`2adec54`) was compared with the pre-lock
  parent (`477f957`) on the same Windows host. Segment eviction measured
  `239.94 us` `[235.58, 246.31] us` post-lock versus `231.85 us`
  `[228.94, 235.72] us` pre-lock in the first matched run. Threaded small
  cycles measured `7.411 us` `[6.8026, 7.8406] us` post-lock versus `8.558 us`
  `[8.0492, 9.2929] us` pre-lock. Cross-thread small handoff measured
  `14.120 us` `[13.303, 14.826] us` post-lock versus `15.441 us`
  `[15.062, 15.815] us` pre-lock; Criterion reported `p = 0.09`. A second
  post-lock segment run measured `303.30 us` `[293.29, 314.26] us`, exposing
  host contention/noise on this row. PR #9 changes more than the lock, so these
  are provider-state comparisons, not lock-only attribution. The reclamation
  lock is retained; no safe optimization is justified without a lock-isolated
  harness.

- [patch] status=done owner=codex scope=`crates/mnemosyne-local`,
  allocator regression tests, and PM artifacts; root-cause and eliminate the
  Miri-confirmed alloc/free page-metadata aliasing violation recorded in
  `gap_audit.md`. Acceptance: the Hermes reproducer passes under both Stacked
  Borrows and Tree Borrows, focused Mnemosyne value-semantic tests pass under
  nextest, and the fix introduces no allocator-cycle threshold regression.
- [patch] status=done owner=codex scope=`D:/atlas/worktrees/mnemosyne-ritk`
  on branch `codex/mnemosyne-0.2-page-provenance`; port the Miri-verified
  page-provenance correction onto the exact 0.2 provider line consumed by RITK.
  Acceptance: focused allocator nextest and Clippy pass, RITK pins the verified
  revision, and its registration wheel completes without a native crash.
  Rejected after audit: RITK already pins `477f957`, whose parent is the exact
  Miri-verified correction `5a9f49f`; no consumer pin change is required.
- [x] [patch] status=done owner=codex scope=`crates/mnemosyne-local`,
  `crates/mnemosyne-benchmarks`, and performance PM artifacts;
  last-update=2026-07-15. Investigate the remaining
  `allocator deallocation latency/large_8192` gap to RpMalloc. The matched
  default-feature Criterion row measures Mnemosyne `36.960 ns`
  `[33.540, 38.661] ns` versus RpMalloc `6.1139 ns`
  `[5.8441, 6.5791] ns` (`6.05x`). `8192` is the maximum small class, and
  the opt-in branch probe plus value-semantic regression establish that the
  same-owner single-block case commits through `InPlaceSmall`, with no large/
  huge classifier or full-page relink. No production mutation is justified:
  the measured row does not enter the page-list transition candidates, and
  the remaining gap is comparator implementation difference rather than an
  identified Mnemosyne correctness or contention defect.

## Next

- [x] [patch] Complete the Miri page-metadata provenance fix before resuming the
  RpMalloc deallocation-gap investigation. The merged provider head already
  contains `5a9f49f`; the exact Hermes regression passes under both Miri
  aliasing models and the RITK 0.2 pin was verified against that correction.
