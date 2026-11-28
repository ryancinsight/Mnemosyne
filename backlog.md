# Backlog

<a id="mnemosyne-stable-qualification"></a>
## MNEMOSYNE-STABLE-QUALIFICATION — mnemosyne stable-release qualification [patch] — todo
- priority: verification; milestone: M-core-allocation; basis: `2326baa3ff15`.
- Outcome: Resolve the allocator rescue queue against the current released source. Follow the [release scope](docs/adr/0014-release-scope-1-0.md) and [milestone exits](backlog/milestones.md). Planning targets for `mnemosyne-memory` are 0.11.0, then 0.12.0, then 1.0.0. Verification alone can advance the current version.
- Acceptance and sequence: [stable-release checklist](checklist.md#stable-release-checklist).

## Ready

<a id="MN-RESCUE-QUEUE"></a>
- [ ] [correctness] **MN-RESCUE-QUEUE — port rescue PR #190 onto main in slices.**
  priority: correctness; milestone: M-core-allocation; status: todo; needs: none; scope: the work at
  `refs/pull/190/head` (released 2026-11-10).
  **Outcome:** each #190 hunk is ported or proven obsolete; then delete `rescue/mnemosyne-audit-20260928`.
  **Acceptance:** after `git fetch origin pull/190/head`, each path of `git diff --name-status
  FETCH_HEAD~1 FETCH_HEAD` is ported to main or rejected with its reason in the closing PR.
  **Next step:** one slice per PR, at most 400 changed lines; move main's files, never copy #190's older bodies.
  Unported: build-util tests split
  with `emit_nightly_tls_cfg()` -> `nightly_tls_directives() -> Vec<&'static str>` and its three `build.rs` callers (public
  break of published `mnemosyne-build-util` 0.2.1: [major] ADR first); `warm_current_thread` into `allocator.rs` with
  reshaped `pub use`; `#![deny(missing_docs)]` on `mnemosyne-benchmarks`; `DefaultBackend` leaf renames and README text (classify by cargo-semver-checks; the leaf modules are private, so no public path changes); delete `checklist.md` after folding its open entries (MN-SCRATCH-RELEASE-2026-09-03 :62-68, the Apollo retention and warm-pass boxes; MN-436 :70-81; packed tagged pool :83-88) into item blocks.
  Obsolete, reject with this reason: #190's rustfmt-only hunks, already on main or in files main removed or rewrote (`tls/os_helpers.rs`, `tls/native.rs`).
  basis: origin/main d9ddf96d.

<a id="MN-RESCUE-REFS"></a>
- [ ] [correctness] **MN-RESCUE-REFS — dispose of rescue refs that have no open PR or item.**
  priority: correctness; milestone: M-core-allocation; status: todo; needs: none; scope: the refs below.
  **Outcome:** each ref is ported to main, shown landed (`git merge-tree --write-tree origin/main <head>` prints main's tree),
  or released with its branch deleted.
  **Acceptance:** none of `rescue/mnemosyne-audit-round2-extension`, `rescue/mnemosyne-219-whole-block-frees` and
  `rescue/mnemosyne-book-closure` remains on origin, and main or a pull ref reaches each; PR #240 is ported
  or rejected with a reason (PR #250's benchmark budgets are ported).
  **Next step:** diff each against main: `rescue/mnemosyne-audit-round2-extension` (sibling commit of #190 on the same parent;
  3 files differ from it), `rescue/mnemosyne-219-whole-block-frees` (28 files, +728/-121 since its merge-base with main),
  `rescue/mnemosyne-book-closure` (#240).
  basis: origin/main d8480efd.

<a id="MN-RESCUE-PRS"></a>
- [ ] [correctness] **MN-RESCUE-PRS — classify released PRs #216, #218, #219, #220, #222 and #223.**
  priority: correctness; milestone: M-core-allocation; status: todo; needs: none; scope: `refs/pull/<N>/head` of each listed PR.
  **Outcome:** each pull ref is shown landed on main, ported, or rejected with a reason; no released PR lacks a citing item.
  **Acceptance:** for each head, `git merge-tree --write-tree origin/main <head>` prints main's tree, or the closing PR cites its port or rejection.
  **Next step:** compare `rescue/mnemosyne-219-whole-block-frees` with #218 and #219 (it holds a commit titled like #219 whose head is not its ancestor), then the rest.
  basis: origin/main 75ee517b.

<a id="MN-LOCAL-MIRI-TREE"></a>
### MN-LOCAL-MIRI-TREE — make `global_alloc_tests` Tree-Borrows-clean and run it in the CI Miri job — todo
  priority: correctness; milestone: M-core-allocation; needs: none; scope: `crates/mnemosyne-local/src/free/classified.rs`,
  `crates/mnemosyne-local/src/local_alloc/page/allocation.rs`, `crates/mnemosyne/tests/global_alloc_tests/`, `.github/workflows/ci.yml`.
  **Outcome:** any program that allocates through `Mnemosyne` is Miri-clean under Tree Borrows. At the basis Tree Borrows rejects the page-metadata read at `free/classified.rs:101` and the `alloc_count` update at `local_alloc/page/allocation.rs:57`,
  and `ci.yml:498-522` runs only `counting_allocator` from the facade. ADR 0012 accepts the registry through which `locate_segment` reaches page metadata; its code is at `refs/pull/220`, `/218`, `/219`, `/222` and the CI step at `/216`, the same refs MN-RESCUE-PRS classifies.
  **Reproduce:** `MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-tree-borrows" cargo +nightly miri test -p mnemosyne-memory --test global_alloc_tests -- basic::test_basic_allocation`.
  **Acceptance:** with the `MIRIFLAGS` above, `cargo +nightly miri nextest run -p mnemosyne-memory --profile miri -E 'binary(global_alloc_tests) & not test(/^leak::/) & not test(=policy::test_cuda_backends)'` passes at the default seed and `-Zmiri-seed=1` to `4` (ADR 0012, Miri fact 2),
  and a step of the CI `miri` job runs it: `git grep -nF "binary(global_alloc_tests) & not test(/^leak::/) & not test(=policy::test_cuda_backends)'" -- .github/workflows/ci.yml` prints one line (nothing at the basis).
  Only those two exclusions are admitted: `leak::` until MN-PROF-MIRI-FRAMES, and `policy::test_cuda_backends`, which reaches `dlopen` (`crates/mnemosyne-backend/src/backends/cuda/loader.rs:88`) through `is_cuda_available` (`policy.rs:80`);
  that Miri lacks `dlopen` on Linux is the claim of the step comment at `refs/pull/216/head`, not reproduced here. `cargo +nightly miri nextest list -p mnemosyne-memory -E 'binary(global_alloc_tests)'` (same `MIRIFLAGS`) names every test that `cargo nextest list -p mnemosyne-memory -E 'binary(global_alloc_tests)'` names at the basis (list recorded in the closing PR),
  and `git grep -nE "cfg(_attr)?[!(].*miri|#\[ignore" -- crates/mnemosyne/tests/global_alloc_tests crates/mnemosyne/tests/global_alloc_tests.rs crates/mnemosyne-local/src/free/classified.rs crates/mnemosyne-local/src/local_alloc/page/allocation.rs` prints nothing (nothing at the basis), so neither a test nor the scoped free path leaves the run through a Miri gate.
  `git grep -n "stay out until MN-LOCAL-MIRI-UB" -- .github/workflows/ci.yml` prints nothing (`ci.yml:502` at the basis): the `ci.yml:498-507` comment no longer keeps the facade's other tests out.
  **Next step:** port the registry slices in the ADR's order, one PR each of at most 400 changed lines, then the CI step.
  basis: origin/main 9dc65dd0263f00bc8fb7ee7dcfde2d06e51feb78.

<a id="MN-PROF-MIRI-FRAMES"></a>
- [ ] [correctness] **MN-PROF-MIRI-FRAMES — keep captured frame provenance in `mnemosyne-prof`.**
  priority: correctness; milestone: M-core-allocation; status: todo; needs: MN-LOCAL-MIRI-TREE; scope:
  `crates/mnemosyne-prof/src/sampler/`, `.github/workflows/ci.yml`.
  **Outcome:** the sampler stores captured frames as pointers rather than `usize`, so `backtrace::resolve` receives the provenance `frame.ip()` carried.
  Once MN-LOCAL-MIRI-TREE lets `global_alloc_tests` run under Miri, `leak::test_leak_detector_integration` reaches Undefined Behavior under Tree Borrows (carried from the earlier item, not re-run; its Stacked Borrows run belongs to MN-LOCAL-MIRI-UB)
  in `backtrace`'s `miri_resolve_frame`, called from `sampler/report.rs:133` with an address that has no provenance.
  **Acceptance:** with `MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-tree-borrows"`, `cargo +nightly miri nextest run -p mnemosyne-memory --profile miri -E 'binary(global_alloc_tests) & not test(=policy::test_cuda_backends)'` passes, so `leak::` runs
  (MN-LOCAL-MIRI-TREE's `policy::test_cuda_backends` exclusion stays), and `cargo +nightly miri nextest list -p mnemosyne-memory -E 'binary(global_alloc_tests)'` names `leak::test_leak_detector_integration`.
  The `miri` job's Tree Borrows step runs that filter: `git grep -nF "binary(global_alloc_tests) & not test(=policy::test_cuda_backends)'" -- .github/workflows/ci.yml` prints one line (nothing at the basis) and `git grep -n "leak::" -- .github/workflows/ci.yml` prints nothing.
  MN-LOCAL-MIRI-TREE's `cfg`/`ignore` grep still prints nothing. That run also guards the sampler's TLS copy-out fix, which only `leak::` reaches under Miri.
  **Next step:** replace the interner's `[usize]` frame type with a pointer newtype, keeping its hash and the shard selection unchanged.

<a id="MN-LOOM-STACK-MODEL"></a>
- [ ] [verification] **MN-LOOM-STACK-MODEL — state in the loom model's doc that it over-approximates the locked stack.**
  priority: verification; milestone: M-core-allocation; status: todo; needs: none; scope:
  `crates/mnemosyne-arena/tests/loom_tagged_stack.rs`, `crates/mnemosyne-arena/src/segment/pool/tagged_stack/tests.rs`.
  **Outcome:** the module doc says what the model is. Every shipped mutator holds `mutation_lock`
  (`tagged_stack.rs:144,162,178,233`), so no two overlap; the model's `pop` and both tests
  (`a_popped_node_carries_the_link_its_pusher_published`, `concurrent_pops_never_hand_out_the_same_node`) run unlocked
  threads, a sound over-approximation of the locked stack (a lock-faithful model would serialize the mutators and have nothing to explore).
  The doc calls pop "the lock-free part" (`loom_tagged_stack.rs:10`), which reads as a claim about the shipped stack.
  **Acceptance:** the module doc states that every shipped mutator holds `mutation_lock`, that the model drops the lock on purpose,
  and that a pass therefore covers the locked stack; `len` (`tagged_stack.rs:53`) is named as the one unlocked reader;
  `RUSTFLAGS="--cfg loom" cargo test -p mnemosyne-arena --test loom_tagged_stack --release` passes; `tagged_stack/tests.rs:178-179` stays true.
  **Next step:** reword the module doc; no model change.
  basis: origin/main 315eab70b0c863abb1c6e1d36bafa1768bdd6783.

<a id="MN-MIRI-STRESS-BOUND"></a>
- [ ] [verification] **MN-MIRI-STRESS-BOUND — state the miss rate the Miri stress size supports.**
  priority: verification; milestone: M-core-allocation; status: todo; needs: none; scope:
  `crates/mnemosyne-arena/src/segment/pool/tagged_stack/tests.rs`.
  **Outcome:** the doc of `STRESS_PAIRS_PER_WORKER` (`tests.rs:160-180`) gives a one-sided upper bound on the probability that one Miri seed misses a stack with the lock and the head tag removed,
  from a stated seed count and confidence, in place of "250 sits 2.5x above the point where detection saturates". It records 12 of 12 seeds detecting at 250 pairs and names the seeds.
  With zero misses in `k` independent seeds the per-seed miss probability is below `1 - α^(1/k)` at confidence `1 - α` (`k = 12`, `α = 0.05` gives 0.22; `k = 299` gives 0.01).
  **Acceptance:** the doc states `k`, `α`, the seeds and the bound; a Miri run of the same mutant over those seeds fails the conservation assertion (`tests.rs:224-227`) on every one, recorded in the closing PR; the constant changes only if the stated bound is unacceptable for a stated reason.
  **Next step:** choose `k` and `α`, then run the mutant over that many `-Zmiri-seed` values.
  basis: origin/main 7f2d1b55130119790953a773b59a854b4f18c936.

<a id="MN-CI-DRAFT-READY"></a>
### MN-CI-DRAFT-READY — skip heavy CI jobs on draft PRs and run CI when a PR becomes ready — todo
  priority: verification; milestone: M-core-allocation; needs: none; scope: `.github/workflows/ci.yml`, `.github/workflows/msrv.yml`.
  **Outcome:** a draft PR whose head has `code=true` in `ci.yml`'s classifier (a path matching `crates/*`, `fuzz/*`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `deny.toml`, `scripts/*` or `.github/workflows/*`, `ci.yml:73`) runs no heavy job and fails `CI aggregate`; marking it ready starts the full run.
  At the basis `ci.yml:4` and `msrv.yml:12` list no `types`, so `ready_for_review` starts no run, and no job of either file tests `pull_request.draft`. A `code=true` draft head runs `verify`, `supply-chain`, `loom`, `miri`, `test-aarch64`, `benchmark-hygiene` and `test-tsan` (`ci.yml:115,205,230,295,527,576,601`);
  `lockfile` and `semver` also run when `manifest` is `'true'` (`ci.yml:92,104`), which `ci.yml:78` sets only for `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `deny.toml` and `.github/workflows/*`. `msrv.yml`'s `check` runs on a draft touching its `paths` (`msrv.yml:13-18`: `.github/workflows/msrv.yml`, `Cargo.lock`, `Cargo.toml`, `crates/**`, `rust-toolchain.toml`),
  not one touching only `scripts/*` or `ci.yml`; a head touching none of the `ci.yml` paths skips the nine jobs behind `changes` through the classifier (`changes` and `aggregate` still run). `adr-index.yml:32` carries the gate; `msrv.yml` is in scope.
  **Acceptance:** `python -c "import yaml;[print(f,(yaml.safe_load(open(f))[True]['pull_request'] or {}).get('types'),[j for j,v in yaml.safe_load(open(f))['jobs'].items() if j not in ('changes','aggregate') and 'draft == false' not in str(v.get('if'))]) for f in ('.github/workflows/ci.yml','.github/workflows/msrv.yml')]"`
  prints the four types `opened`, `synchronize`, `reopened`, `ready_for_review` and `[]` for both files (`None` and nine names, then `None` and `['check']`, at the basis); and `python -c "import yaml;print(any('pull_request.draft' in str(s.get('if')) and 'exit 1' in str(s.get('run')) for s in yaml.safe_load(open('.github/workflows/ci.yml'))['jobs']['aggregate']['steps']))"` prints `True` (`False` at the basis);
  on a draft PR touching `crates/**`, `verify`, `miri`, `loom` and `test-tsan` conclude `skipped` and `CI aggregate` fails, and marking it ready starts a run (event `pull_request`, action `ready_for_review`) in which they run; both run links in the closing PR.
  **Next step:** add the `types` list to both workflows, `github.event.pull_request.draft == false` (the form of `adr-index.yml:32`) to each job's `if:`, and a failing draft step to `aggregate`.
  basis: origin/main 9dc65dd0263f00bc8fb7ee7dcfde2d06e51feb78.

<a id="MN-HUGE-POOL-TEST-SEGMENT"></a>
### MN-HUGE-POOL-TEST-SEGMENT — build the test `Segment` once, with no integer-to-pointer cast — todo
  priority: verification; milestone: M-core-allocation; needs: none; scope: `crates/mnemosyne-arena/tests/huge_pool_concurrency.rs`,
  `crates/mnemosyne-arena/tests/segment_pool_concurrency.rs`, `crates/mnemosyne-arena/src/segment/tests_huge_pool.rs`, `gap_audit.md`.
  **Outcome:** one function boxes a zeroed `Segment` and calls `Segment::initialize` for these tests; three do at the basis: `tests_huge_pool.rs:53-66`
  (`core::ptr::without_provenance_mut(raw)`, `:58`), `huge_pool_concurrency.rs:24-37` and `segment_pool_concurrency.rs:19-29`, both with `raw as *mut u8` (`:30`, `:27`);
  the first also widens `block_size as usize` (`:84`). Its module doc says "The repo has no `loom` harness" (`huge_pool_concurrency.rs:10`), false since `tests/loom_tagged_stack.rs` and the CI `loom` job (`ci.yml:227`).
  **Acceptance** (P = `crates/mnemosyne-arena/tests crates/mnemosyne-arena/src/segment/tests_huge_pool.rs`): `git grep -cE "Segment::initialize[[:space:]]*\(" -- $P` prints one line, count 1 (three files at the basis);
  `git grep -nE " as " -- $P | grep -vE ":[0-9]+:[[:space:]]*//"` prints nothing (3 lines: the two `raw as *mut u8` and `block_size as usize`); `git grep -nE "no .loom. harness" -- crates` prints nothing (1 line);
  `git diff -U0 origin/main...HEAD -- crates | grep -E "^\+[[:space:]]*pub"` prints nothing; `cargo nextest list -p mnemosyne-arena` names both `*_concurrent_push_pop_conserves_every_segment` tests before and after.
  `git grep -l "Segment::initialize" -- crates/mnemosyne-arena` lists only `src/arena.rs`, `src/arena/tests.rs`, `src/segment/alloc/allocate.rs`, `src/segment/pool/tagged_stack/tests.rs`, `src/segment/tests.rs` and `src/segment/tests_huge_pool.rs` (8 files at the basis; no new builder file);
  `git diff -U0 origin/main...HEAD -- crates/mnemosyne-arena | grep -E "^\+[^+]" | grep -vE "^\+[[:space:]]*//" | grep -E " as "` prints nothing.
  **Next step:** fold both stress tests into in-crate leaves (they use only `GlobalHugePool` and `GlobalSegmentPool`) so the builder in `tests_huge_pool.rs` serves them; a `tests/common` module would keep two builders. `loom_tagged_stack.rs` stays; update the `gap_audit.md:7` link.
  basis: origin/main 9dc65dd0263f00bc8fb7ee7dcfde2d06e51feb78.

<a id="MN-SCRATCH-TEST-STRENGTH"></a>
### MN-SCRATCH-TEST-STRENGTH — make the scratch release and growth tests fail under the defects they name — todo
  priority: verification; milestone: M-core-allocation; needs: none; scope: `crates/mnemosyne-arena/src/scratch/tests.rs`.
  **Outcome:** each mutant below fails the named test. At the basis `ScratchBank::release` and `reset` (`bank.rs:80,91`) have no test caller (the `.release()`/`.reset()` callers in `tests.rs` are `ScratchPool`s, `:712-814`, `:1156`);
  `tests.rs:1148-1152` asserts `>= 12288`; `tests.rs:713` expects 16_384, which growth from 128 to 16_384 already yields (`grow_geometric`, `storage.rs:216`, doubles only when the request is smaller).
  **Acceptance:** each mutant, applied alone under `crates/mnemosyne-arena/src`, makes `cargo nextest run -p mnemosyne-arena` fail the named test, and the unmutated crate passes:
  M1 `ScratchBank::release` returns zeros without calling `pool.release()` -> new `bank_release_reclaims_every_pool_above_provision`;
  M2 `ScratchBank::reset` has an empty body -> new `bank_reset_makes_release_reclaim_every_pool`;
  M3 the bounded path's growth call (`pool/borrow.rs:103`) grows to exactly `n` -> `scratch_pool_bounded_path_release_trims_to_provision`, once `:1148` asserts `slot_capacity(0) == 16384` (from 8192, `max(12288, 2*8192)`);
  M4 `ScratchPool::release` skips `shrink_to(provision)` (`pool/manage.rs:48`) and reports live capacities -> `pool_release_reclaims_above_provision`, once its sizes are `[128, 10_000, 16_384]` (capacity 20_000 against provision 16_384; a release returning `[0; MAX_POOL_SLOTS]` is already killed by `tests.rs:713`, `:734-735`). Each kill is recorded in the closing PR.
  **Next step:** add the two bank tests and change the two assertions.
  basis: origin/main 9dc65dd0263f00bc8fb7ee7dcfde2d06e51feb78.

<a id="MN-PROCESS-TAG-CITES"></a>
### MN-PROCESS-TAG-CITES — drop comments that cite board items whose blocks are gone — todo
  priority: verification; milestone: M-core-allocation; needs: none; scope: the seven lines named below.
  **Outcome:** no comment names a deleted board ID, and each keeps its rationale. Sites at the basis: `crates/mnemosyne-arena/src/scratch/aligned_vec/length.rs:33`, `.../scratch/pool/borrow.rs:98` and `.../scratch/tests.rs:1126,1130` (MN-SCRATCH-GROWTH-COST),
  `.github/workflows/ci.yml:143` and `scripts/safety_ratchet.py:10` (MNEM-UNSAFE-DOC-1), `ci.yml:467` (MN-459).
  **Acceptance:** `git grep -nE "MN-459|MNEM-UNSAFE-DOC-1|MN-SCRATCH-GROWTH-COST" -- . ":!backlog.md"` prints nothing (7 lines at the basis); `git grep -c amortized -- crates/mnemosyne-arena/src/scratch/aligned_vec/length.rs crates/mnemosyne-arena/src/scratch/pool/borrow.rs crates/mnemosyne-arena/src/scratch/tests.rs`
  still prints counts of at least 3, 2 and 2; `git diff -U0 origin/main...HEAD -- crates .github | grep -E "^[+-]" | grep -vE "^(\+\+\+|---)" | grep -vE "^[+-][[:space:]]*(//|#)"` prints nothing, so only comment lines change; `git grep -cE "baseline lives|lowers .BASELINE. in the same change|own test helpers pass both borrow" -- .github/workflows/ci.yml scripts` still prints `ci.yml:2` and one `scripts/` file with count 1 (the rationale of `ci.yml:143`, `:467` and `safety_ratchet.py:10` stays).
  **Next step:** reword each comment to state its reason without the ID.
  basis: origin/main 9dc65dd0263f00bc8fb7ee7dcfde2d06e51feb78.

<a id="MN-CI-WASM-CHECK"></a>
### MN-CI-WASM-CHECK — compile the wasm32 backend in CI — todo
  priority: verification; milestone: M-core-allocation; needs: none; scope: `.github/workflows/ci.yml`, `docs/adr/0011-wasm-memory-backend.md`, and the crate the check names if it fails at the basis.
  **Outcome:** a CI job compiles `mnemosyne-backend` and `mnemosyne-local` for `wasm32-unknown-unknown`, so the wasm32 backend (`crates/mnemosyne-backend/src/backends/mod.rs:19-22`) and the environment-reader gate (`crates/mnemosyne-local/src/options.rs:3`) cannot regress unseen.
  At the basis `git grep -n wasm32 -- .github` prints nothing: no job builds any crate for that target. ADR 0011 (`:51`) records one `cargo check --target wasm32-unknown-unknown --offline`; its `Board item:` line (`:7`) links the removed block `backlog.md#mn-wasm-2026-09-06`.
  **Acceptance:** `cargo check --locked --target wasm32-unknown-unknown -p mnemosyne-backend -p mnemosyne-local` exits 0, and the same command on a tree without `backends/mod.rs:19-22` exits nonzero (`mapping.rs:19`, compiled unconditionally by `lib.rs:36`, imports `DefaultBackend` from `lib.rs:43`; both runs recorded in the closing PR);
  a `ci.yml` job runs it after `rustup target add wasm32-unknown-unknown` and is a `needs` entry of `aggregate` (`git grep -n "wasm32-unknown-unknown" -- .github/workflows/ci.yml` prints the install and the check; `ci.yml:650` lists the job);
  `git grep -n "backlog.md#mn-wasm" -- docs` prints nothing (`docs/adr/0011-wasm-memory-backend.md:7` at the basis) and ADR 0011's Verification section names the job.
  **Next step:** add the job gated on `code=true`; `cargo check` proves compilation, not link-safety (no host `getenv` import), which needs a linked artifact and is outside this item.
  basis: origin/main 9dc65dd0263f00bc8fb7ee7dcfde2d06e51feb78.

## Blocked

<a id="atlas-mnemosyne-stage-d1"></a>
- [ ] [minor] **ATLAS-MNEMOSYNE-STAGE-D1 — branded device buffers.**
  status=blocked; milestone: M-policies-backends; owner=external integration; last-update=2026-09-04.
  Blocker: the Mnemosyne `MemoryBackend`, Melinoe lifetime brands, and
  Hephaestus device/stream completion contract do not yet share a branded
  asynchronous ownership boundary. Re-open when that provider seam and a
  concrete Coeus consumer target exist; no downstream adapter.
<a id="mnem-provider-publish-1"></a>
- [ ] [patch] **MNEM-PROVIDER-PUBLISH-1 — publish provider crates.**
  status=blocked; milestone: M-core-allocation; owner=external; last-update=2026-09-04. Re-open when
  crates.io contains the required Eunomia and Melinoe versions and Themis has
  a released source-aligned dependency graph.
<a id="mnem-tagged-pool-pack-1"></a>
- [ ] [perf-experiment] **MNEM-TAGGED-POOL-PACK-1 — compare packed pool state.**
  status=blocked; milestone: M-core-allocation; owner=codex; last-update=2026-09-04. Re-open on a coherent
  native Windows toolchain and a quiet Criterion run; compare warm-pool,
  handoff, eviction, and retention rows before changing cache-line layout.
<a id="ar-4"></a>
- [ ] [patch] **AR-4 — strengthen benchmark gate statistics.**
  status=blocked; milestone: M-core-allocation; owner=codex; last-update=2026-09-04. Re-open on a quiet
  machine for the paired sampling change and threshold baseline refresh.

<a id="MN-LOCAL-MIRI-UB"></a>
### MN-LOCAL-MIRI-UB — run the facade under Stacked Borrows and let `counting_allocator` wrap `Mnemosyne` under Miri — blocked
  priority: correctness; milestone: M-core-allocation; needs: MN-LOCAL-MIRI-TREE, MN-PROF-MIRI-FRAMES; scope: `crates/mnemosyne/tests/global_alloc_tests/`, `crates/mnemosyne/tests/counting_allocator.rs`, `.github/workflows/ci.yml`.
  **Blocker:** rust-lang/miri#2686, "Refresh provenance of global allocator" (`gh issue view 2686 -R rust-lang/miri --json title,state` prints state OPEN at the basis). Under Stacked Borrows the facade's `dealloc` and in-place `realloc`
  receive std's narrowed pointers and write past them (ADR 0012, Miri fact 3), so facade Stacked Borrows coverage stays zero. **Re-open trigger:** the same command prints state CLOSED.
  **Outcome:** `global_alloc_tests` passes Miri under Stacked Borrows with `leak::` included (it needs MN-PROF-MIRI-FRAMES; only `policy::test_cuda_backends` stays out, as in MN-LOCAL-MIRI-TREE), and `counting_allocator` wraps `Mnemosyne` under Miri, dropping its `cfg(miri)` `System` inner (`counting_allocator.rs:24,28,35`).
  **Acceptance:** `MIRIFLAGS=-Zmiri-disable-isolation cargo +nightly miri nextest run -p mnemosyne-memory --profile miri -E 'binary(global_alloc_tests) & not test(=policy::test_cuda_backends)'` passes and `cargo +nightly miri nextest list` with that filter names `leak::test_leak_detector_integration`;
  the CI `miri` job has that step (`git grep -nF "binary(global_alloc_tests) & not test(=policy::test_cuda_backends)'" -- .github/workflows/ci.yml` prints two lines: the Tree Borrows step of MN-PROF-MIRI-FRAMES and this one);
  `git grep -nE "cfg\((not\()?miri" -- crates/mnemosyne/tests/counting_allocator.rs` prints nothing (3 lines at the basis); the comments citing this ID (`ci.yml`, `counting_allocator.rs:8-13`) are rewritten: `git grep -n "MN-LOCAL-MIRI-UB" -- crates .github` prints nothing.
  **Next step:** none until the blocker clears; then add the step and run the acceptance command.
  basis: origin/main 9dc65dd0263f00bc8fb7ee7dcfde2d06e51feb78.
