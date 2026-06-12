# Checklist

Target version: 0.2.0

## Verified

- [x] [patch] Split `mnemosyne-prof` TLS provider and per-thread hook state
  machinery into `src/tls.rs`, leaving `src/lib.rs` focused on public control
  APIs and allocation/free hook entry points. No Rust file remains over 500
  lines; `lib.rs` is 231 lines and `tls.rs` is 285. Verification:
  `cargo test -p mnemosyne-prof -- --test-threads=1`; `cargo fmt --check`;
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
  `cargo nextest run --workspace --all-features`; `cargo test --doc
  --workspace --all-features`; `cargo doc --workspace --all-features
  --no-deps`; `cargo run -p mnemosyne-benchmarks --features
  system-jemalloc --bin benchmark_summary -- --enforce-thresholds`; `git diff
  --check`.
- [x] [patch] Split `mnemosyne` global allocator integration coverage into
  `basic`, `stats`, `realloc`, `policy`, and `leak` leaf modules. The
  integration-test root now owns only the global allocator, shared imports, and
  module wiring; largest leaf is 312 lines. Verification: focused
  `cargo test -p mnemosyne --test global_alloc_tests -- --test-threads=1`;
  `cargo fmt --check`; `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`; `cargo nextest run --workspace
  --all-features`; `cargo test --doc --workspace --all-features`; `cargo doc
  --workspace --all-features --no-deps`; `cargo run -p
  mnemosyne-benchmarks --features system-jemalloc --bin benchmark_summary --
  --enforce-thresholds`; `git diff --check`.
- [x] [patch] Replace the ad hoc local allocator TLS seed cache with
  `melinoe::thread_cached!`, making Melinoe the SSOT for thread-cached
  initialization while preserving the nonzero randomized seed contract.
  Verification: `cargo fmt --check`; focused `mnemosyne-heap` unit tests;
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
  `cargo nextest run --workspace --all-features`; `cargo test --doc
  --workspace --all-features`; `cargo doc --workspace --all-features
  --no-deps`; `cargo run -p mnemosyne-benchmarks --features
  system-jemalloc --bin benchmark_summary -- --enforce-thresholds`; `git diff
  --check`.
- [x] [patch] Split `mnemosyne-heap` unit tests into `heap`, `boxed`, `cell`,
  `vec`, and `traits` leaf modules. The root test module is now 40 lines and
  the largest leaf is 255 lines, preserving the same value-semantic coverage
  under the deep vertical hierarchy target. Verification: same gate as above.
- [x] [patch] Remove the remaining command-argument `Vec` allocation from
  `benchmark_summary` by parsing `--refresh-baseline` and
  `--enforce-thresholds` in one pass over `std::env::args()`. Verification:
  value-semantic flag parser tests; `cargo fmt --check`; focused
  `benchmark_summary` tests; `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`; `cargo nextest run --workspace
  --all-features`; `cargo test --doc --workspace --all-features`; `cargo doc
  --workspace --all-features --no-deps`; `cargo run -p
  mnemosyne-benchmarks --features system-jemalloc --bin benchmark_summary --
  --enforce-thresholds`; `git diff --check`.
- [x] [patch] Refresh `benchmarks/allocator_comparison.md` from a complete
  `system-jemalloc` allocator Criterion run and investigate the only initial
  gated regression. `segment cache eviction/mnemosyne` first reported
  `278577.994 ns` with unstable variance; focused rerun stabilized it at
  `249453.566 ns`, ratio `1.076` against the selected baseline and below the
  `1.15` gate. Verification: `cargo bench -p mnemosyne-benchmarks --features
  system-jemalloc --bench allocator_bench`; focused `Segment cache eviction`
  reruns; `cargo run -p mnemosyne-benchmarks --features system-jemalloc --bin
  benchmark_summary -- --enforce-thresholds`; `git diff --check`.
- [x] [patch] Split the `benchmark_summary` binary into leaf modules for
  allocator comparison rendering, active-group config, CSV parsing, Criterion
  extraction, metadata writing, report writing, and threshold policy; removed
  tracked unreferenced `scratch/test.*` artifacts; and made report/metadata
  writers create missing parent directories. Evidence tier: value-semantic
  writer test; `cargo fmt --check`; focused `benchmark_summary` tests;
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`;
  `cargo nextest run --workspace --all-features`; `cargo test --doc
  --workspace --all-features`; `cargo doc --workspace --all-features
  --no-deps`. `benchmark_summary -- --enforce-thresholds` was also exercised:
  it now reaches threshold validation and fails because this checkout has no
  current Criterion rows under `target/criterion`, not because of a writer path
  error.
- [x] [patch] Add default `parallel` and `mnemosyne-memory` feature contracts
  to every Mnemosyne crate; facade `mnemosyne-memory` forwards to the existing
  branded heap-backed memory surface. Verification: `cargo metadata --no-deps
  --locked --format-version 1`; full Atlas feature-policy metadata audit;
  `cargo fmt --check`; `cargo check --workspace --locked`; `cargo test
  --workspace --locked`; `cargo clippy --workspace --all-targets --locked
  -- -D warnings`; `cargo doc --workspace --no-deps --locked`; `git diff
  --check`. Native allocator benchmark dependencies require Ninja/Clang on
  this windows-gnu host because the MSYS GCC frontend rejects the target flags.
- [x] [patch] Route `mnemosyne-local::current_cpu_id` through
  `themis::current_processor()` so Themis owns processor identity and
  Mnemosyne consumes the topology provider instead of duplicating Linux/Windows
  probes. Evidence tier: provider integration through full workspace gate.
- [x] [patch] Add stable `std_tls` feature routing for the local allocator and profiler TLS selectors, re-export `MemoryBackendWrapper`/`LocalAllocatorSelector` through `mnemosyne`, and remove the duplicate top-level import/header that made Apollo clippy fail through the local Mnemosyne patch. Verification: `cargo fmt --check`; `cargo check --workspace --all-features`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --all-features`; `cargo doc --workspace --all-features --no-deps`; `git diff --check`.
- [x] [minor] Add provider-owned `ScratchBank<T, const N>` for fixed scratch-role banks, preserving zero-copy `ScratchPool` slot semantics while reducing Apollo-side repeated thread-local pool declarations. Verified by `scratch_bank_slots_are_independent`, full scratch unit subset, `cargo check -p mnemosyne-arena`, clippy, and docs.
- [x] [patch] Prevent the combined usable-size benchmark from cross-optimizing allocation, query, and deallocation by passing the allocated pointer through `black_box` before the `usable_size` call and before `dealloc`; apply the same consumed `black_box` pattern to the allocator-cycle helper. Verified by focused Criterion: `usable size latency/Mnemosyne/small/32` `2.307 ns`, `medium/1024` `2.350 ns`, `large/8192` `5.196 ns`, and regenerated `allocator_comparison.md` reports small `2.297 ns`, medium `2.340 ns`, large `5.206 ns`.
- [x] [patch] Route `GlobalAlloc::dealloc` through `thread_free_layout`, stamp owner allocator cache pointers, bypass the busy-bit write pair for first frees from full pages, move full pages back to active pages with one branded list token, and add active `rpmalloc::RpMalloc` benchmark coverage. Verified by local/global allocator tests and `benchmark_summary --features system-jemalloc -- --enforce-thresholds`; refreshed comparison reports `allocator deallocation latency/large_8192` Mnemosyne `40.909 ns` versus RpMalloc `6.871 ns` (`5.95x`) and `allocator cycle latency/large_8192` `2.136 ns`.
- [x] [patch] Remove the remaining intermediate `Vec` allocation from `benchmark_summary` Criterion path normalization, allocator-comparison map keys, and markdown row formatting by using borrowed benchmark slices plus copy-sized `Display` adapters that stream into the existing output buffers. Verified by `normalize_path_joins_components_without_intermediate_vec` and the jemalloc-enabled benchmark summary gate.
- [x] [patch] Make `mnemosyne-prof` dump reporting borrow active samples shard-by-shard instead of cloning `Sample` values and exact stack slices into snapshot vectors; folded profile generation now builds stack keys directly in reverse frame order, and leak reporting streams each sample directly to the report file while `Path::to_string_lossy` stays scoped as a `Cow`. Verified by focused profiler tests and clippy.
- [x] [patch] Remove duplicate post-`alloc_cold` defrag accounting from `thread_alloc_cold`. Verified by `thread_alloc_cold_charges_one_defrag_operation_per_page_refill`; retained benchmark summary reports `allocator allocation latency/large_8192` Mnemosyne `42.252 ns` versus RpMalloc `19.026 ns`, with threshold gate passing after a focused large-cycle rerun.
- [x] [patch] Add GhostCell-style branded page-list mutation tokens around active/full/empty intrusive page-list push and unlink operations. Runtime representation remains raw page pointers plus ZST/`PhantomData` tokens; focused Criterion reports cycle rows small `2.717 ns`, medium `2.684 ns`, large `2.640 ns`, and `benchmark_summary --enforce-thresholds` passes.
- [x] [patch] Add GhostCell-style branded owned-segment mutation tokens around owned-list push/unlink operations. Runtime representation remains raw `*mut Segment` links plus ZST/`PhantomData` tokens; Miri verifies `owned_segment_list_is_doubly_linked_and_unlinks_in_place`, focused segment-cache rerun reports `239.81 us`, and `benchmark_summary --enforce-thresholds` passes.
- [x] [patch] Carry one branded page-list token through `pop_best_empty_page` traversal and unlink. Focused burst-retention rows improved to small `591.601 ns`, medium `1052.330 ns`, and large `2686.985 ns`; segment-cache eviction stayed ungated and summary threshold enforcement passes.
- [x] [patch] Reject page-local pop/bump helper consolidation. Focused Criterion with the helper in `alloc_class` regressed `allocator allocation latency/mnemosyne/small_32` by `+35.518%` and cycle rows by `+9.047%` small / `+7.020%` large; retaining it only in the cold active-head retry still made `benchmark_summary --enforce-thresholds` reject small `1.173x` and large `1.056x`.
- [x] [patch] Reject current-segment minimum-block free shortcut. Focused Criterion showed the broad variant regressed `allocator deallocation latency/mnemosyne/medium_1024` by `+27.336%`; the narrowed `MIN_BLOCK_SIZE` variant still regressed medium `+44.173%` and large `+51.250%` deallocation rows. Reverted.
- [x] [patch] Reject skipping fresh-page `initialize_free_list` under `StandardPolicy`; the experiment improved `allocator allocation latency/large_8192` to `41.486 ns` but regressed cycle thresholds small `1.068x`, medium `1.109x`, and large `1.143x`. Reverted path passes; final regenerated target rows report allocation `42.252 ns` and deallocation `36.985 ns` for `large_8192`.
- [x] [patch] Reject direct full-page relink because a full active page is not linked into `full_pages` until the next cold allocation; reject layout-aware small free because it regressed deallocation rows (`large_8192` measured `53.270 ns` during the experiment).
- [x] [patch] Restore exact RpMalloc classifier/report columns in `benchmark_summary`, regenerating `benchmarks/allocator_comparison.md` with `RpMalloc (ns)` and `Mnemosyne vs RpMalloc` columns.
- [x] [patch] Reject active-page empty-`thread_free` guards after `benchmark_summary --enforce-thresholds` reported cycle regressions during the experiment: small `1.151x`, medium `1.066x`, and large `1.612x` over baseline. Reverted path passes with final ratios small `1.005x`, medium `0.921x`, and large `1.021x`.
- [x] [patch] Gate `nightly_tls` on the active compiler channel so stable `--all-features` builds do not enable unstable `#[thread_local]`.
- [x] [patch] Add `RUSTC` as a build-script rerun input for every `nightly_tls_active` cfg generator.
- [x] [patch] Maintain `ThreadAllocator::owned_segment_count` through owned-segment insert/remove/reclaim paths, replacing repeated cold-path threshold scans with O(1) metadata. Verified by owned-list invariant assertions and workspace tests.
- [x] [patch] Split `local_alloc::segment` into `segment::ownership` and `segment::reclaim` leaf modules while preserving the existing allocator API surface.
- [x] [patch] Replace cross-thread handoff benchmark per-iteration `Vec` allocation with a fixed per-worker buffer, run the `system-jemalloc` comparison, and refresh the selected benchmark baseline. Verified by full Criterion run, detached-`HEAD` comparison, and threshold enforcement.
- [x] [patch] Remove remote-free defrag counter charging from the non-owner allocator. Verified by `cross_thread_free_does_not_charge_non_owner_defrag_counter`, owner-side remote-free reclamation, and `cross-thread free handoff/mnemosyne/small_32` at `0.525x` of baseline.
- [x] [patch] Replace threaded allocation-cycle worker vectors with fixed arrays. Verified by `threaded small allocation cycles/mnemosyne` at `4.529 us` with stable variance and the jemalloc-enabled threshold gate.
- [x] [patch] Optimize `usable_size` small-allocation page-index derivation. Verified by focused usable-size tests, `usable size latency/mnemosyne/small_32` at `2.821 ns`, and stable variance.
- [x] [patch] Move thread-local allocator telemetry into `local_alloc::stats` and compute snapshots from active/full/empty page lists instead of scanning every page in every owned segment. Verified by focused stats-list invariants and local allocator tests.
- [x] [arch] Consolidate public heap construction to the scoped `Heap<'brand, P, B>` API, delete the duplicate explicit/branded heap public types, and keep `RawHeap<P, B>` as the single internal allocator implementation.
- [x] [patch] Remove `MnemosyneHeap`/`BrandedHeap` allocator-comparison columns and regenerate `benchmarks/allocator_comparison.md` with real SnMalloc `huge_2m` rows.
- [x] [minor] Implement `HardenedPolicy` ZST with XOR-encoded free-list `next` pointers (key per page from a TLS seed). Layer over `SecurePolicy`.
- [x] [minor] Add unit test coverage for `HardenedPolicy` round-trip and pointer-tamper detection.

- [x] [patch] Close re-entrancy soundness hole on the guard-free fast path via `with_allocator_unguarded` (busy-bit checked, guard-write-free). Verified: stable + `nightly_tls` green; `unguarded_fast_path_rejects_reentrant_borrow` pins re-entry rejection.
- [x] [patch] Reduce `unlink_owned_segment` to O(1) via an intrusive doubly-linked owned-segments list (`Segment::prev_owned_segment`, SSOT `push_owned_segment`). Verified: `owned_segment_list_is_doubly_linked_and_unlinks_in_place`.
- [x] [arch] Add `complexity_audit.md` per-component complexity review with O(1) reduction plan for the remaining cold-path unlink operations.
- [x] [minor] Add an optional `nightly_tls` `#[thread_local]` fast cache accessor to `mnemosyne-local`, preserving thread-exit reclamation via a `Drop` sentinel; default stable build unchanged. Verified: stable workspace `cargo test` green (no regression); nightly `cargo test -p mnemosyne-local --features nightly_tls` green (18 tests, incl. sentinel reclamation).
- [x] [patch] Preserve current segment ownership during local free reclamation checks.
- [x] [patch] Benchmark Mnemosyne against mimalloc and snmalloc for allocation cycles.
- [x] [patch] Benchmark burst allocation retention without heap-allocated benchmark setup vectors.
- [x] [patch] Benchmark threaded small allocation cycles across the same allocator set.
- [x] [patch] Run `cargo fmt --all -- --check`.
- [x] [patch] Run `cargo test --workspace`.
- [x] [patch] Run `cargo bench -p mnemosyne-benchmarks --bench allocator_bench -- --quick`.
- [x] [patch] Expose `mnemosyne::memory_stats()` with mapped-byte and retained-segment counters.
- [x] [patch] Bound the global reusable segment pool by `PAGES_PER_SEGMENT`.
- [x] [patch] Add cross-thread free handoff benchmarks for 32-byte and 1024-byte layouts.
- [x] [patch] Skip segment-reclaim calls on hot local frees for the current segment.
- [x] [patch] Expose current-thread live allocations and owned segment counts in `mnemosyne::memory_stats()`.
- [x] [patch] Expose process-wide cross-thread reclaimed block count.
- [x] [patch] Add and run `cargo run -p mnemosyne-benchmarks --bin memory_report --release`.
- [x] [patch] Replace cross-thread free benchmark thread spawn with persistent bounded-channel workers.
- [x] [patch] Expose per-size-class occupancy telemetry.
- [x] [patch] Print per-size-class occupancy rows from `memory_report`.
- [x] [patch] Replace threaded allocation benchmark thread spawn with persistent bounded-channel workers.
- [x] [patch] Add `Segment cache eviction` Criterion benchmark.
- [x] [patch] Print `eviction_after` memory telemetry from `memory_report`.
- [x] [patch] Expose purged segment, purge call, and purged byte telemetry.
- [x] [patch] Add `benchmark_summary` command.
- [x] [patch] Run `cargo run -p mnemosyne-benchmarks --bin benchmark_summary --release`.
- [x] [patch] Add `purge_after` row to `memory_report`.
- [x] [patch] Generate `benchmarks/allocator_baseline_excerpt.csv`.
- [x] [patch] Filter compact benchmark summary to active Criterion groups.
- [x] [patch] Add benchmark baseline metadata.
- [x] [patch] Generate `target/criterion/benchmark_baseline_comparison.csv`.
- [x] [patch] Keep `thread_free` segment metadata available after small-allocation classification.
- [x] [patch] Test benchmark summary CSV parsing and baseline ratio computation.
- [x] [patch] Restore memory retention-bound test syntax.
- [x] [patch] Stabilize page-recycling test around segment reuse and size-class metadata.
- [x] [patch] Prevent default benchmark summary runs from mutating the source-controlled baseline.
- [x] [patch] Route cross-thread frees through page-local atomic free lists.
- [x] [patch] Remove duplicate small-free segment metadata derivation.
- [x] [patch] Verify eventual page-local remote-free reclamation without adding a hot-path atomic check.
- [x] [patch] Move remote-free drain/count/link logic into `Page::reclaim_thread_free`.
- [x] [patch] Test `Page::reclaim_thread_free` with concrete block identity and allocation count assertions.
- [x] [patch] Compile global allocator calls through the `StandardPolicy` ZST policy path.
- [x] [patch] Remove production `align_up` panic path in favor of `checked_align_up`.
- [x] [patch] Replace hot-path `expect`/`unwrap` invariant sites in `ThreadAllocator::alloc`, `alloc_cold`, `get_new_page`, and `try_recycle_page` with `debug_assert!` + `core::hint::unreachable_unchecked`.
- [x] [patch] Drop stale `align_up` re-export from `mnemosyne-arena::lib` so the workspace compiles cleanly.
- [x] [patch] Keep quick-mode benchmark summary extraction non-gating unless `--enforce-thresholds` is passed.
- [x] [patch] Keep generated benchmark metadata under `target/criterion`.
- [x] [patch] Stabilize page-recycling test assertions under reusable segment state.
- [x] [patch] Remove benchmark-summary test-build dead-code warning.
- [x] [patch] Add fine-grained regression threshold checks for selected Mnemosyne benchmark rows.
- [x] [patch] Derive hard regression threshold policy from repeated non-quick benchmark samples and define per-benchmark thresholds.
- [x] [patch] Compare page-queue cross-thread handoff results against mimalloc and snmalloc in a side-by-side table.
- [x] [patch] Audit remaining test and benchmark panic sites and verify that production library code contains zero panic paths.
- [x] [patch] Factor allocation initialization and free poisoning into inlined policy helpers.
- [x] [patch] Add a test-module lock around allocator integration tests that share global segment-pool counters.
- [x] [patch] Apply GhostCell-style owner/data separation with a transparent `SegmentOwner` token.
- [x] [patch] Remove the allocator-level incoming free queue and route re-entrant frees through page-local queues.
- [x] [patch] Test re-entrant local free fallback through the page-local atomic queue.
- [x] [patch] Complete backend-specific arena segment-pool typing and update telemetry call sites.
- [x] [patch] Reject single-TLS local-free rewrite after focused benchmark regression.
- [x] [patch] Reject `UnsafeCell` allocator permission split after focused cycle benchmark regression.
- [x] [patch] Add saturated threaded small-allocation benchmark coverage for scheduler-overhead isolation.
- [x] [patch] Give each `LocalAllocatorSelector` backend implementation distinct thread-local storage.
- [x] [patch] Run `cargo bench -p mnemosyne-benchmarks --bench allocator_bench -- "Threaded saturated small allocation cycles" --quick`.
- [x] [patch] Expose page-refill, recycle, fresh-page, fresh-segment, orphan-adoption, and recycle-sweep telemetry.
- [x] [patch] Defer owned-segment recycle sweeps until the current segment has no unsliced pages.
- [x] [patch] Run `cargo bench -p mnemosyne-benchmarks --bench allocator_bench -- "Allocator burst retention/Mnemosyne/small/32" --quick`.
- [x] [patch] Run `cargo bench -p mnemosyne-benchmarks --bench allocator_bench -- "Threaded saturated small allocation cycles" --quick`.
- [x] [patch] Reject local-free TLS collapse after `Threaded small allocation cycles/Mnemosyne` exceeded the configured threshold.
- [x] [patch] Replace the gated threaded baseline with `threaded saturated small allocation cycles/mnemosyne`.
- [x] [patch] Test that the historical threaded row is not part of the threshold-gated baseline.
- [x] [patch] Run `cargo run -p mnemosyne-benchmarks --bin benchmark_summary --release -- --enforce-thresholds`.
- [x] [patch] Remove panic assertions and unwrap/expect calls from `allocator_bench.rs` and `memory_report.rs`.
- [x] [patch] Run panic-pattern scan for benchmark runner and memory report.
- [x] [patch] Add `# Safety` contracts to production allocation initialization and free-poisoning helpers.
- [x] [patch] Add local safety comments for benchmark dynamic symbol casts, unchecked layouts, allocator calls, and segment-cache cycles.
- [x] [patch] Audit backend-specific CUDA unified-memory tracking for bounded metadata and zero-cost fallback behavior.
- [x] [patch] Synchronize README architecture notes with page-local remote-free routing and CUDA fallback behavior.
- [x] [patch] Audit production unsafe blocks in `mnemosyne-backend` for local safety contracts and ordering minimality.
- [x] [patch] Change `MemoryBackend::deallocate` to return a release-success boolean and defer `current_mapped_bytes` decrements to confirmed success across unix, windows, CUDA, and the `MemoryBackendWrapper` telemetry path.
- [x] [patch] Add `failed_release_increments_call_count_without_byte_delta` test pinning the failure-path accounting contract.
- [x] [patch] Keep failed arena purge releases retained in the segment pool and count only confirmed releases in purge telemetry.
- [x] [patch] Add `purge_retains_segment_when_backend_release_fails` coverage for arena purge failure accounting.
- [x] [patch] Make `MemoryBackend::deallocate` `#[must_use]` and document explicit ignored-result handling for unrecoverable cleanup contexts.
- [x] [patch] Change `deallocate_large_or_huge` to return backend release status for huge mappings.
- [x] [patch] Add `huge_deallocation_returns_backend_release_status` coverage for large/huge release-result propagation.
- [x] [patch] Retain full-pool segment mappings when direct backend release fails during segment deallocation.

- [x] [patch] Document and `debug_assert!` the pointer-alignment, reserved-prefix, and payload-bound invariants for the huge-allocation metadata slot.
- [x] [patch] Add `huge_allocation_metadata_slot_round_trips_across_alignments` covering align in {1, 2, 4, 8, 16, 64, 4 KiB, 64 KiB, 1 MiB}.
- [x] [patch] Reject non-power-of-two large-allocation alignments before backend allocation.
- [x] [patch] Reject large-allocation alignments above `SEGMENT_SIZE` so free classification can recover the header by segment rounding or metadata-slot lookup.
- [x] [patch] Add `huge_allocation_rejects_alignment_above_segment_size` coverage.
- [x] [patch] Document the `thread_free` classifier invariant and debug-check that small frees never target metadata page 0.
- [x] [patch] Add `debug_assert!` checks for `page_index < PAGES_PER_SEGMENT`, `page.block_size > 0`, and block-stride alignment in the small-free classifier.
- [x] [patch] Add `small_alloc_returns_block_aligned_ptr_outside_metadata_page` covering 8 B–1 KiB requests with mixed alignments.
- [x] [patch] Reject zero, non-power-of-two, and above-segment alignments in `thread_alloc` before size-class or arena routing.
- [x] [patch] Reject zero alignment in `allocate_large_or_huge`.
- [x] [patch] Add `thread_alloc_rejects_invalid_alignment_requests` coverage.
- [x] [patch] Reject zero-size direct `thread_alloc` and `allocate_large_or_huge` requests.
- [x] [patch] Return null for zero-size `GlobalAlloc::alloc` calls in `Mnemosyne` and generic `MnemosyneAllocator`.
- [x] [patch] Add zero-size rejection coverage for local, arena, and global allocator entry points.
- [x] [patch] Add `MAX_ALLOC_SIZE` as the shared pointer-offset-safe payload bound.
- [x] [patch] Reject direct `thread_alloc` requests above `MAX_ALLOC_SIZE`.
- [x] [patch] Reject arena mappings whose total backend mapping requirement exceeds `MAX_ALLOC_SIZE`.
- [x] [patch] Add size-bound rejection tests for local and arena allocation entry points.
- [x] [patch] Split global allocation routing through `thread_alloc_layout` for `Layout`-validated hot-path requests.
- [x] [patch] Release local allocator test allocations and serialize shared-state tests to keep workspace verification deterministic.

- [x] [patch] Extract `is_valid_alloc_request` and `is_valid_layout_alloc_request` `const fn` predicates in `mnemosyne-core::validation`.
- [x] [patch] Replace per-clause `size`/`align` checks in `thread_alloc`, `thread_alloc_layout`, and `allocate_large_or_huge` with the centralized validators.
- [x] [patch] Add value-semantic coverage for each validator clause in `mnemosyne-core::validation::tests`.

- [x] [patch] Tighten huge-allocation backend mapping to `size + alignment + SEGMENT_ALIGN + PAGE_SIZE`, eliminating the `SEGMENT_SIZE`-of-slack overshoot in the prior derivation.
- [x] [patch] Add `huge_allocation_consumes_tight_mapping_size` to pin the new mapping formula via backend telemetry deltas.

- [x] [patch] Remove the dead `Page::segment` back-pointer field and unused `Page::is_empty` helper.
- [x] [patch] Document the no-back-pointer rationale and drop the per-page `segment` write loop from `Segment::initialize`.
- [x] [patch] Add `page_struct_size_stays_within_one_cache_line` to pin `size_of::<Page>() <= 64`.
- [x] [patch] Keep current-segment occupancy-mask bits conservative after local frees and pin the contract with `current_segment_free_keeps_occupancy_mask_conservative`.
- [x] [patch] Replace the shifted-mask page-index derivation in `usable_size` with an offset-from-segment-base derivation and refresh the small usable-size comparator row.

## Open

- [x] [patch] Add `threaded medium allocation cycles/` to the benchmark-summary active group filter so Criterion rows from `allocator_bench.rs` are retained in generated summaries and comparison reports.
- [x] [patch] Add benchmark-summary unit coverage pinning active allocator benchmark groups and rejecting exploratory TLS rows from allocator comparison summaries.
- [x] [patch] Make `benchmark_summary -- --enforce-thresholds` fail when any selected baseline row is missing from current Criterion data.
- [x] [patch] Document `benchmark_variance.csv` and the selected-row completeness gate in the benchmark workflow.
- [x] [patch] Treat `threaded medium allocation cycles/` as a threaded variance row so scheduler-width classification matches the retained benchmark group.
- [x] [patch] Make allocator comparison classification exact so `MnemosyneHeap` and `BrandedHeap` rows cannot overwrite the public `Mnemosyne` row.
- [x] [patch] Replace the remaining benchmark harness `expect` in the `BrandedHeap` cycle row with explicit `benchmark failure` diagnostics.
- [x] [patch] Consolidate `MnemosyneHeap` and `BrandedHeap` allocation/free/realloc mechanics behind one internal `RawHeap<P, B>` implementation.
- [x] [patch] Keep branding as type-level ownership evidence around shared heap mechanics instead of a second allocator algorithm.
- [x] [patch] Remove the top-level `mnemosyne::MnemosyneHeap` re-export so explicit heaps live at the `mnemosyne_heap` boundary.

- [x] [patch] Audit generated benchmark artifact freshness and documentation references for the current allocator comparison set.
- [x] [patch] Document the source-controlled baseline versus generated `target/criterion` artifact boundary.
- [x] [patch] Update benchmark metadata wording for the active `--enforce-thresholds` gate and current saturated threaded comparator sample.

- [x] [patch] Add value-semantic diagnostic messages to bare `assert!` invocations in `mnemosyne`, `mnemosyne-arena`, `mnemosyne-backend`, and `mnemosyne-local` tests so failure output names the unexpected value.
- [x] [patch] Replace bare test `unwrap()` calls with contextual `expect()` diagnostics in allocator, local allocator, and channel/thread test code.
- [x] [patch] Serialize arena allocation tests that inspect process-wide backend telemetry so exact mapped-byte assertions remain deterministic.
- [x] [patch] Document the `size_to_class(0)` zero-size mapping contract and add `size_class_boundaries_are_exact` plus `size_class_zero_maps_to_smallest_class` coverage at every piecewise transition.
- [x] [patch] Extract `try_reclaim_and_allocate` helper that folds the three "drain remote frees → record → pop free block → bump alloc_count" sites in `ThreadAllocator::alloc` and `alloc_cold` into a single `#[inline(always)]` routine.

- [x] [patch] Audit production debug assertions for value-semantic invariant messages and zero-cost release behavior.
- [x] [patch] Add value-semantic messages to production `debug_assert!` checks while preserving debug-only code generation.
- [x] [patch] Verify no predicate-only `debug_assert!` sites remain in production crates.

- [x] [patch] Audit local allocator remote-free reclaim paths for duplicated block-pop logic.
- [x] [patch] Refresh selected Mnemosyne threshold-gated Criterion rows and regenerate `target/criterion` summaries.

- [x] [patch] Investigate full all-allocator Criterion quick-run timeout while focused gated rows complete.
- [x] [patch] Replace unsupported `--quick` benchmark invocation with an explicit bounded Criterion smoke configuration.
- [x] [patch] Make `unlink_full_page` return whether the page was actually removed and only re-activate pages after a confirmed full-list unlink.

- [x] [patch] Audit benchmark baseline metadata after bounded Criterion harness configuration.
- [x] [patch] Refresh `benchmarks/allocator_baseline_excerpt.csv` from a complete bounded Criterion run and verify `--enforce-thresholds`.
- [x] [patch] Confirm `thread_free` uses `LocalAllocatorSelector::get_allocator_ptr` for the owner-token check without opening the TLS allocator cell.
- [x] [patch] Add jemalloc to allocator benchmark comparator coverage where `tikv-jemallocator` is linkable.
- [x] [patch] Extend allocator comparison report generation with Jemalloc value and ratio columns.

- [x] [patch] Add compile-time `const _: () = assert!(...)` invariants in `mnemosyne-core::constants` pinning `SEGMENT_SIZE`/`PAGE_SIZE` power-of-two, `SEGMENT_ALIGN == SEGMENT_SIZE`, `PAGE_ALIGN == PAGE_SIZE`, `PAGES_PER_SEGMENT * PAGE_SIZE == SEGMENT_SIZE`, `PAGES_PER_SEGMENT >= 2`, `MAX_SMALL_ALLOC_SIZE <= PAGE_SIZE`, `MAX_ALLOC_SIZE >= SEGMENT_SIZE`, and `NUM_SIZE_CLASSES > 0`.
- [x] [patch] Add compile-time cross-checks in `mnemosyne-core::size_class` that `class_to_size(NUM_SIZE_CLASSES - 1) == MAX_SMALL_ALLOC_SIZE` and `class_to_size(NUM_SIZE_CLASSES) == 0`.
- [x] [patch] Extract `unlink_page_from_list` helper that folds the three linked-list traversal blocks in `unlink_full_page` and `unlink_page` into a single inlined routine taking the list head slot and the target page pointer.

- [x] [patch] Sprint A: Add Linux `MADV_HUGEPAGE` hint in `UnixBackend::allocate` for segment-sized mappings; gate to `target_os = "linux"`; advisory failure ignored.
- [x] [patch] Sprint A: Add `segment_sized_allocation_survives_hugepage_hint` and `sub_segment_allocation_skips_hugepage_hint` Linux-gated regression tests.

- [x] [minor] Sprint A: Add `MemoryBackend::page_reset(ptr, size) -> bool` trait method with default `false` impl.
- [x] [minor] Sprint A: Implement `page_reset` on `UnixBackend` (Linux `MADV_DONTNEED`, macOS/FreeBSD `MADV_FREE`).
- [x] [minor] Sprint A: Implement `page_reset` on `WindowsBackend` via `VirtualAlloc(MEM_RESET)`.
- [x] [minor] Sprint A: Add `page_reset_calls`/`page_reset_bytes` telemetry to `BackendMemoryStats` and `MemoryStats`; wire through `MemoryBackendWrapper`.
- [x] [minor] Sprint A: Add three regression tests pinning page-reset telemetry semantics and round-trip behavior on an active mapping.

- [x] [minor] Sprint A wire-through: Add `reset_segment_pool` arena function that drains the retained pool, calls `page_reset` on each cached segment, and pushes them back.
- [x] [minor] Sprint A wire-through: Add `reset_segments` / `reset_calls` telemetry to `ArenaMemoryStats` and `mnemosyne::MemoryStats`.
- [x] [minor] Sprint A wire-through: Add public `mnemosyne::reset()` and `mnemosyne::reset_generic<B>()` APIs.
- [x] [minor] Sprint A wire-through: Add `test_reset_keeps_segments_cached_and_records_telemetry` integration test pinning the reset/purge separation and telemetry deltas.

- [x] [minor] Sprint B seam: Add `MemoryBackend::make_guard(ptr, size) -> bool` trait method with default `false` impl.
- [x] [minor] Sprint B seam: Implement `make_guard` on `UnixBackend` (`mprotect(PROT_NONE)`).
- [x] [minor] Sprint B seam: Implement `make_guard` on `WindowsBackend` (`VirtualProtect(PAGE_NOACCESS)`).
- [x] [minor] Sprint B seam: Add `guard_install_calls`/`guard_install_bytes` telemetry to `BackendMemoryStats` and `MemoryStats`; wire through `MemoryBackendWrapper`.
- [x] [minor] Sprint B seam: Add three regression tests pinning guard-install telemetry semantics, confirmed-install + reservation persistence, and null/zero rejection.
- [x] [patch] Sprint B tail guard: add opt-in `mnemosyne-arena/segment-tail-guards` feature and install a 4 KiB guard in fresh-segment tail slack only when enabled.
- [x] [patch] Sprint B tail guard: add feature-gated regression coverage for exact tail-guard address and size.
- [x] [patch] Extend `memory_report` with page-reset, guard-install, reset-segment, and reset-call telemetry plus a `reset_after` phase.
- [x] [patch] Mark `size_to_class` and `class_to_size` as `#[inline(always)]` so downstream allocator crates receive the piecewise mapper body for monomorphized hot paths.
- [x] [patch] Move secure-policy small-free poisoning after small-page classification to avoid the duplicate page lookup in the poisoned free path.
- [x] [patch] Reject layout-aware `GlobalAlloc::dealloc` small-free classification after `Threaded saturated small allocation cycles/Mnemosyne` regressed to about `248.94 us`.
- [x] [patch] Refresh `benchmarks/allocator_comparison.md` after the current cycle and saturated threaded benchmark runs.

- [x] [minor] Sprint B wire-through: Add `SEGMENT_TAIL_GUARD_SIZE = 4096` constant with compile-time `is_power_of_two` and slack-bound checks.
- [x] [minor] Sprint B wire-through: Install a guard region at `aligned_addr + SEGMENT_SIZE` via `B::make_guard` only when `mnemosyne-arena/segment-tail-guards` is enabled; default builds compile out the guard path.
- [x] [minor] Sprint B wire-through: Add `fresh_segment_install_increments_guard_telemetry_and_round_trips` test pinning the guard-install delta and clean post-release telemetry.

- [x] [minor] Add `mnemosyne_local::usable_size(ptr)` returning the size-class block size for small allocations, the payload remainder for huge allocations, and 0 for null.
- [x] [minor] Re-export `usable_size` from the top-level `mnemosyne` crate alongside `SizeClassOccupancy`.
- [x] [minor] Add `usable_size_returns_block_size_for_small_allocations`, `usable_size_returns_payload_remainder_for_huge_allocations`, and `usable_size_returns_zero_for_null_pointer` regression tests.
- [x] [minor] Add `Usable size latency` Criterion coverage for Mnemosyne, mimalloc, snmalloc, and target-gated jemalloc.
- [x] [patch] Add `usable size latency/` to generated benchmark summary and allocator comparison reports.
- [x] [patch] Optimize `usable_size` small-pointer classification by reading the target page block size before falling back to huge metadata.
- [x] [patch] Extend huge usable-size coverage across 8 B, 64 KiB, 1 MiB, and segment-aligned huge allocations.

- [x] [minor] Add `GlobalAlloc::realloc` override on `Mnemosyne` that returns `ptr` unchanged when `new_size <= usable_size(ptr)`.
- [x] [minor] Add equivalent `GlobalAlloc::realloc` override on the generic `MnemosyneAllocator<P, B>` so standard policy allocations skip the alloc+copy+free round trip when the request fits in the current class.
- [x] [patch] Preserve `SecurePolicy` zero-initialization by forcing replacement allocation for secure realloc growth even when the request fits in the current usable block.
- [x] [minor] Add regression tests for in-place realloc: same-pointer for within-class grow/shrink, copy semantics across classes, null-to-alloc, zero-size-to-free, secure replacement growth, and zero-size null realloc.
- [x] [minor] Add `Realloc latency` Criterion coverage for within-class and cross-class realloc cycles across Mnemosyne, mimalloc, snmalloc, and target-gated jemalloc.
- [x] [patch] Add `realloc latency/` to generated benchmark summary and allocator comparison reports.
- [x] [minor] Add `Usable size query latency` Criterion coverage for isolated metadata-query cost across Mnemosyne, mimalloc, snmalloc, and target-gated jemalloc.
- [x] [patch] Add `usable size query latency/` to generated benchmark summary and allocator comparison reports.
- [x] [minor] Add `Allocator allocation latency` Criterion coverage with drop-guard cleanup for allocation-only attribution across Mnemosyne, mimalloc, snmalloc, and target-gated jemalloc.
- [x] [patch] Add `allocator allocation latency/` to generated benchmark summary and allocator comparison reports.
- [x] [minor] Add `std::alloc::System` comparator rows for allocation-only, cycle, burst, realloc, cross-thread handoff, and saturated threaded allocator benchmark groups.
- [x] [patch] Extend generated allocator comparison reports with System value and Mnemosyne-vs-System ratio columns.
- [x] [patch] Optimize small-free classification by reading the target page's block size before the huge-allocation metadata fallback.
- [x] [patch] Remove duplicate TLS lookup from local-free owner checks by deriving the current allocator token inside the existing allocator-cell access.
- [x] [minor] Add `Allocator deallocation latency` Criterion coverage with setup-allocated pointers so the measured routine isolates `dealloc`.
- [x] [patch] Add `allocator deallocation latency/` to generated benchmark summary and allocator comparison reports.
- [x] [patch] Remove dead `Page::local_free` metadata and the allocation fast-path branch that checked it.
- [x] [patch] Refresh allocator comparison rows after `Page::local_free` removal.
- [x] [patch] Add standard-policy small-realloc size-class proof fast path before the `usable_size` fallback.
- [x] [patch] Refresh selected mimalloc-regression rows: threaded small allocation cycles, usable size latency/small_32, threaded saturated small allocation cycles, and realloc latency/within_class_24_to_32.
- [x] [patch] Add current-segment local-free fast path that bypasses allocator-cell mutable borrow when the free does not require page-list relinking or non-current segment reclaim.
- [x] [patch] Refresh threaded small and saturated small allocation rows after the current-segment local-free fast path.
- [x] [minor] Add `LocalAllocatorSelector::with_allocator_guard` with a macro override that combines re-entrancy guard management and allocator access.
- [x] [patch] Refresh threaded small and saturated small allocation rows after allocation guard TLS consolidation.
- [x] [patch] Replace hot-path `size_to_class` arithmetic with a compile-time lookup table.
- [x] [minor] Replace thread-local allocator `RefCell` access with guarded `UnsafeCell` access under the allocation flag.
- [x] [patch] Add `target/criterion/benchmark_variance.csv` generation with relative mean confidence-interval width and threaded-row variance thresholds.
- [x] [patch] Refresh allocator cycle, realloc, usable-size, and threaded rows after the size-class and TLS allocator-access optimizations.

- [x] [patch] Fix `usable_size` over-report for huge allocations: use `segment.raw_alloc_ptr + huge_size` as the mapping end instead of `segment_ptr + huge_size` (which sits up to `SEGMENT_ALIGN - 1` bytes past the OS mapping boundary).
- [x] [patch] Fix the equivalent over-report in `thread_free`'s `SecurePolicy` poisoning sizing on both the segment-aligned and the fallback huge-allocation paths.
- [x] [patch] Add `usable_size_does_not_over_report_past_mapping_end_for_huge_allocations` strict assertion test.

- [x] [patch] Extract `Segment::huge_mapping_suffix_from(user_ptr) -> usize` helper centralizing the `raw_alloc_ptr + huge_size - ptr` derivation.
- [x] [patch] Replace the four duplicated formula sites (`usable_size` segment-aligned and fallback; `thread_free` `SecurePolicy` poison on both branches) with the helper.
- [x] [patch] Pin the helper contract with debug assertions for `huge_size > 0` and `user_ptr ∈ [raw_alloc_ptr, raw_alloc_ptr + huge_size]`.
- [x] [patch] Add a direct core test proving `Segment::huge_mapping_suffix_from` uses `raw_alloc_ptr` as the mapping base.
- [x] [patch] Reject precomputed-class allocation fast path and direct realloc-capacity formula after Criterion regressions in threaded and realloc rows.
- [x] [patch] Reject layout-aware small-deallocation bypass after `Threaded saturated small allocation cycles/Mnemosyne` regressed.
- [x] [patch] Document that realloc slow paths copy only `min(layout.size(), new_size)` bytes, not size-class slack.

- [x] [patch] Document the `realloc` slow-path copy-length contract on both `Mnemosyne` and `MnemosyneAllocator<P, B>`: copy is `min(layout.size(), new_size)` because the bytes beyond `layout.size()` are size-class slack the user never initialized.
- [x] [patch] Add `test_realloc_does_not_copy_past_layout_size` regression test that writes a sentinel into the 8-byte slack window of an 8 B → 16 B class-0 allocation, performs cross-class realloc, and asserts the slack pattern does not propagate into the new allocation.
- [x] [patch] Collapse the allocator guard and cache into one `LocalAllocatorSlot<B>` TLS key.
- [x] [patch] Run focused Criterion rows for allocator cycle latency, threaded small cycles, and saturated threaded small cycles after the TLS-slot change.
- [x] [patch] Regenerate `allocator_comparison.md` and run `benchmark_summary --release -- --enforce-thresholds`.
- [x] [patch] Reject forced `AtomicFreeList` inlining after it improved cross-thread handoff but regressed saturated threaded cycles.
- [x] [patch] Reject `thread_local!` const initialization for the allocator slot after it improved non-saturated rows but regressed saturated threaded cycles.
- [x] [patch] Add all-size-class lower-bound coverage for `usable_size`.
- [x] [patch] Reject separate owner-token TLS routing after cycle latency, cross-thread handoff, and saturated threaded rows regressed.

## Next

- [x] [patch] Harden global allocator leak-detector integration test with guarded profiler/allocation cleanup and contextual dump diagnostics.
- [x] [patch] Replace bare policy integration test layout/thread-join unwraps with contextual diagnostics.
- [x] [patch] Harden local topology tests with contextual lock/layout/segment diagnostics and an RAII guard for the global per-CPU cache flag.
- [x] [patch] Replace the remaining bare C-shim leak-report `CString` unwrap with contextual UTF-8 and interior-NUL diagnostics.
- [x] [patch] Reconcile `complexity_audit.md` with the current free-list/bump-page allocator and remove the stale planned bitmap summary-word item.
- [x] [patch] Replace bare segment-layout unwraps in `mnemosyne-core::types` tests with a contextual `segment_layout()` helper.
- [x] [patch] Remove bare `unwrap()`/panic-prone cleanup paths from `mnemosyne-prof` integration tests; add RAII guards for profiler state and thread allocations so failure paths release hooks, profiling/leak-detector state, and live allocations.
- [x] [patch] Remove production panic paths from native TLS-key initialization in `mnemosyne-local` and `mnemosyne-prof`; native TLS allocation failure now falls back to the standard thread-local slot instead of unwinding.
- [x] [patch] Harden profiler sampling against poisoned shard locks and 32-frame stack capture overflow; retained samples still store exact-length `Box<[usize]>` stacks.
- [x] [patch] Collapse the clippy-reported nested occupancy-mask transition branch in `Page::set_alloc_count_for_segment`.
- [x] [patch] Re-run selected baseline Criterion rows and `benchmark_summary -- --enforce-thresholds` under a quiescent benchmark environment; current public `Mnemosyne` selected rows now pass the retained threshold gate against the source-controlled baseline.
- [x] [patch] Preserve profiler sample memory efficiency by retaining exact captured stack slices instead of fixed 32-frame arrays while keeping sharded active-sample maps.
- [x] [patch] Re-run `usable size latency/small_32` after the profiler/heap consolidation cycle; the focused row now measures Mnemosyne `2.450 ns` versus mimalloc `3.342 ns`.
- [x] [patch] Re-run explicit/branded heap cycle rows after heap-core consolidation; `MnemosyneHeap` is now `0.93x`, `0.92x`, and `0.95x` versus public Mnemosyne for small, medium, and large cycle rows.
- [x] [patch] Move `RawHeap` large/huge deallocation into a shared cold helper so public and branded free paths do not duplicate cold branch bodies.
- [x] [patch] Continue variance-aware investigation of `realloc latency/within_class_24_to_32`.
- [x] [patch] Reject `size_to_class_nonzero(MAX_SMALL_ALLOC_SIZE)` boundary special-casing after benchmark feature isolation: focused Criterion improved `allocator cycle latency/large_8192` by `24.038%`, but `benchmark_summary --enforce-thresholds` still reported `allocator cycle latency/small_32` at `1.071x`; source reverted.
- [x] [patch] Replace size-class runtime arithmetic with a generated `SIZE_TO_CLASS: [u8; MAX_SMALL_ALLOC_SIZE + 1]`; focused Criterion improves cycle rows small `21.673%`, medium `21.846%`, and keeps allocation/deallocation rows without significant regression.
- [x] [patch] Continue variance-aware investigation of `threaded small allocation cycles`, `cross-thread free handoff/small_32`, and combined usable-size latency without reintroducing rejected local-free, layout-aware deallocation, forced atomic-queue inlining, const TLS initialization, or separate owner-token TLS paths.
- [x] [patch] Run target-gated jemalloc comparator refresh on a platform where `tikv-jemallocator` links.

- [x] [patch] Add `usable_size_never_under_reports_across_every_size_class` exhaustive lower-bound test covering every small size class at its lower boundary and class max, the analog of the over-report guard.

- [x] [patch] Extract `realloc_copy_grow<A: GlobalAlloc>` shared slow-path helper; route both `Mnemosyne::realloc` and `MnemosyneAllocator::realloc` through it, removing the duplicated allocate/copy/free body.
- [x] [patch] Reject <=128-byte direct realloc-capacity arithmetic after focused benchmarking failed to beat the accepted within-class realloc point estimate and reported an allocator-cycle regression.
- [x] [patch] Mark `realloc_copy_grow<A: GlobalAlloc>` as `#[inline(always)]` so cross-class realloc slow paths keep monomorphized `alloc`/`dealloc` calls at the call site.
- [x] [patch] Refresh realloc and allocator-cycle Criterion rows after the retained realloc helper inlining change.
- [x] [patch] Regenerate `allocator_comparison.md` and run `benchmark_summary --release -- --enforce-thresholds` after the retained change.

- [x] [minor] Sprint C: Add `mnemosyne-c-shim` crate exposing `malloc`/`free`/`calloc`/`realloc`/`aligned_alloc`/`posix_memalign`/`malloc_usable_size` as `extern "C"` with `lib` + `cdylib` crate types.
- [x] [minor] Sprint C: Document the C-vs-Rust realloc copy-length distinction (`min(usable_size, new_size)` for C, since C tracks no requested size) in the shim module docs.
- [x] [minor] Sprint C: Add 11 shim regression tests covering alignment, zero-size, overflow, realloc grow/null/zero, and posix_memalign validation.
- [x] [patch] Reject deferred process-wide cross-thread reclaim telemetry after focused Criterion rows showed no stable small-handoff improvement and regressions in medium handoff plus threaded small allocation cycles.
- [x] [patch] Reject `#[inline(always)]` on `Page::reclaim_thread_free` after a refreshed `Threaded small allocation cycles/Mnemosyne` row regressed to about `16.528 us`.
- [x] [patch] Reject `#[inline(always)]` on exported `mnemosyne_local::usable_size` after focused Criterion rows regressed allocator cycle latency, combined usable-size latency, and raw usable-size query latency.
- [x] [patch] Reject `thread_alloc_layout_small` after focused Criterion rows improved allocation-only small latency to about `12.057 ns` and saturated threaded cycles to about `72.657 us`, but regressed the retained small allocation cycle to about `5.574 ns` and historical threaded small cycles to about `8.650 us`.
- [x] [patch] Serialize backend telemetry tests with a crate-local mutex so process-wide relaxed mapping counters are not compared while sibling tests mutate them.
- [x] [patch] Reject compact `Page` counter layouts (`u16/u8` and `u32`) after both preserved a 48-byte metadata budget but regressed saturated threaded cycles (`~101.720 us` and `~114.240 us`) and/or usable-size latency.
- [x] [patch] Keep `MIN_BLOCK_SIZE = 16` as the single source for the first size-class stride and small-allocation alignment ceiling; remove the stale compact-counter width assertions after compact counters were rejected.
- [x] [patch] Update `Cargo.lock` from `melinoe` `66945f81` / `0.1.0` to `85d498bb` / `0.5.0`; `cargo check -p mnemosyne-heap` passes against the new brand crate.
- [x] [patch] Replace `benchmark_summary` allocator-row `split('/').collect::<Vec<_>>()` plus `to_lowercase()` with borrowed `split_once` parsing, borrowed comparison keys, allocation-free `eq_ignore_ascii_case` classification, and streaming display cells; pin optional sub-benchmark parsing and exact classifier rejection with unit tests.
- [x] [minor] Add `mnemosyne/branded` as a default feature guarding heap branded re-exports, and compile `mnemosyne-benchmarks` against `mnemosyne` with default features disabled; `cargo check -p mnemosyne --no-default-features`, default `cargo check -p mnemosyne`, and heap tests pass.
- [x] [patch] A/B-check `melinoe` update performance impact: latest-lock cycle rows initially regressed, old-lock restored rows, then benchmark-only default-feature isolation restored latest-lock large-cycle behavior enough for targeted follow-up while preserving default branded API.

- [x] [patch] Add `crates/mnemosyne-c-shim/include/mnemosyne.h` C declaration header matching the seven exported `extern "C"` symbols, documenting per-function null/zero/overflow/alignment contracts; reference it from README highlight #13.
- [x] [minor] Sprint C: Add dynamic interposition C demo (`examples/interpose_demo.c`) and dynamic verification build scripts (`run_demo.sh` for Unix, `run_demo.ps1` for Windows) to demonstrate dynamic linking and interposition ABI compliance.

- [x] [patch] Add `smallest_class_page_saturates_without_duplicate_or_early_refill` runtime witness: fill a 16-byte page to its 4096-block capacity, assert `alloc_count == max_blocks` with all-distinct non-null pointers, and confirm the next allocation refills a fresh page rather than returning a duplicate pointer from the full page.

- [x] [patch] Remove the redundant `layout.size() == 0` guard from `Mnemosyne::alloc` and `MnemosyneAllocator::alloc`; `thread_alloc_layout`/`is_valid_layout_alloc_request` already rejects zero-size, so the GlobalAlloc hot path now carries one fewer branch and one fewer copy of the zero-size contract.

- [x] [patch] Reject removing the `MAX_ALLOC_SIZE` predicate from `is_valid_layout_alloc_request`; the focused run improved `Allocator cycle latency/Mnemosyne/small/32` and `Usable size latency/Mnemosyne/small/32`, but regressed `Allocator allocation latency/Mnemosyne/small/32` and `Threaded small allocation cycles/Mnemosyne`.

- [x] [patch] Adopt `const {}` thread-local initializer for `ALLOCATOR_SLOT` (idiomatic stable form; emits the const-init accessor that omits the per-access lazy-init guard branch). Not benchmark-claimed — see gap_audit note on the contended local measurement environment.

- [x] [patch] Add a README "Research Foundations" section mapping each implemented mechanism (sharded free lists, page-local cross-thread queue, segment/page geometry, orphan adoption, decay-style reset, THP hint, guard regions, policy ZSTs) to its source paper/allocator, plus an honest performance-positioning paragraph and the small-alloc gap localization.
- [x] [patch] Reject Bitmap Free Lists for classes 0, 1, and 2 after Criterion small allocation cycles, realloc within-class, usable-size, and threaded allocation benchmarks regressed.
- [x] [patch] Reject Bounded Retention of Huge Mappings and per-CPU cache optimizations after allocator burst retention and threaded cycles regressed.
- [x] [patch] Add zero-sized-type paths for `BrandedBox` and `BrandedVec`, assert they allocate no owned segment, preserve destructor execution, and guard `BrandedVec` capacity growth with checked arithmetic.
- [x] [patch] Extend the ZST path to `BrandedHeap::alloc_init`, `free`, and `free_uninit`; pin with a value-semantic test that `alloc_init::<ZST>` allocates no segment and `free` runs exactly one destructor.
- [x] [patch] Extend the ZST path to `BrandedHeap::realloc`; pin ZST-to-nonzero and ZST-to-zero transitions with allocator-state and destructor-count assertions.
- [x] [patch] Make `BrandedVec::new::<ZST>` use sentinel capacity; pin `len <= capacity`, allocation-free construction, and destructor execution after push/drop.
- [x] [patch] Make `BrandedVec::into_boxed_slice` attempt oversized-storage shrink; pin content preservation and non-increasing usable storage for a 1024-capacity to one-element boxed slice.
- [x] [patch] Add `AllocPolicy::RANDOMIZE_ALLOCATION`; pin randomized page free-list initialization with a seeded permutation test and keep `StandardPolicy` lazy.
- [x] [patch] Route `MnemosyneHeap` and `BrandedHeap` small allocations through `ThreadAllocator::alloc_class`; verify heap and workspace suites.
- [x] [patch] Restore `thread_realloc` shrink-within-class same-pointer behavior; verify `test_realloc_within_class_returns_same_ptr`.
- [x] [patch] Return the same pointer for standard-policy large/huge half-shrink reallocs; pin with `test_realloc_large_half_shrink_returns_same_ptr`.
- [x] [patch] Copy only `min(layout.size(), new_size)` bytes on `thread_realloc` replacement paths; pin secure shrink preservation with `test_realloc_shrink_replacement_copies_only_new_size`.
- [x] [patch] Refresh `realloc latency/Mnemosyne/huge_shrink_4m_to_2m` after the half-shrink fast path; current focused row is `22.405 ns`.
- [x] [patch] Refresh `usable size latency/Mnemosyne/small_32` after the current hot-path stack; current focused row is `2.479 ns`, ahead of the retained mimalloc row.
- [x] [patch] Reduce `mnemosyne-prof` leak/profiling sample stack memory overhead by replacing 32-frame preallocated vectors with fixed-stack capture plus exact-length boxed stack slices.
- [x] [arch] Split `mnemosyne-core` allocator types, `mnemosyne-arena` segment pools/tests, `mnemosyne-local` top-level allocation/free/realloc/TLS/options helpers, `mnemosyne-prof` sampling/reporting, `mnemosyne-c-shim` tests, and `BrandedVec` operations/trait impls into cohesive leaf modules while preserving public re-exports and monomorphized APIs.
- [x] [patch] Refresh `usable size latency/Mnemosyne/small_32` after the leak-detector stack-storage change; current focused row is `2.487 ns` versus mimalloc `2.879 ns`.
- [x] [patch] Stabilize `test_memory_stats_retention_bound` after leak-detector integration by asserting the live-allocation delta created and released by the test, not an absolute baseline invalidated by orphan adoption.
- [x] [arch] Split `mnemosyne-heap` into `heap`, `brand`, `branded_heap`, `branded_box`, `branded_vec`, and test modules; preserve the existing public re-export surface.
- [x] [arch] Split `mnemosyne-local::local_alloc` into `page`, `routing`, `segment`, and test modules without changing the monomorphized `ThreadAllocator` API.
- [x] [patch] Move global allocator tests from `mnemosyne/src/lib.rs` into `crates/mnemosyne/tests/global_alloc_tests.rs`.
- [x] [patch] Remove stale imports introduced by local allocator module splitting; verify `cargo check --workspace` warning-clean for touched allocator modules.
- [x] [patch] Fix decay engine thread-spawning shadowing bug and add `decay_purger_reaches_steady_state` integration test.
- [x] [patch] Expose `get_options` and `configure` in the top-level `mnemosyne` crate and verify via programmatic configuration tests.
- [x] [patch] Add `multi_heap_isolates_allocation_streams` and `multi_heap_release_does_not_touch_other_heaps` integration tests.

- [x] [patch] Consolidate public allocator periodic-defragmentation accounting behind `ThreadAllocator::record_defrag_operation`, keeping the sweep cold and shared across allocation/free paths.
- [x] [patch] Reject applying the shared defrag-accounting helper to `RawHeap` after explicit/branded heap cycle rows showed measurable regressions; keep heap-local hot defrag accounting inline.
- [x] [patch] Re-run focused Mnemosyne hot rows after public defrag-accounting consolidation: small cycle no regression, small usable-size combined improved, saturated threaded improved, and threshold summary passed.
- [x] [patch] Specialize page allocation-counter transitions with increment/decrement helpers and known-index free paths; focused Criterion reports small cycle `2.952 ns`, usable-size combined `3.089 ns`, threaded small `6.076 us`, and saturated threaded `86.402 us`.
- [x] [patch] Refresh `benchmarks/allocator_comparison.md` after focused reruns; public small deallocation now beats listed comparators, while large deallocation remains behind jemalloc.
- [x] [patch] Refresh stale remaining comparator rows after occupancy-counter specialization; current comparison closes small burst retention (`666.657 ns` vs mimalloc `871.779 ns`) and within-class realloc (`4.228 ns` vs mimalloc `4.483 ns`), while cross-class realloc and public small cycle remain active targets.
- [x] [patch] Replace same-owner small cross-class realloc's closure guard with raw allocator-pointer routing and explicit `is_allocating` scope; focused Criterion reports cross-class realloc `8.002 ns` vs mimalloc `10.793 ns` and within-class realloc `3.120 ns` vs mimalloc `5.161 ns`.
- [x] [patch] Bound periodic defragmentation segment-count scans to the four-segment reclaim threshold.
- [x] [patch] Iterate segment reclaim/defragmentation over `page_occupied_mask`; mostly empty segments now visit only occupied pages.
- [x] [patch] Relax hot OS TLS-key loads to `Ordering::Relaxed`; focused Criterion reports small cycle `2.951 ns` vs mimalloc `2.734 ns`, cross-class realloc `6.383 ns` vs mimalloc `7.646 ns`, and saturated threaded small `70.191 us` vs mimalloc `79.338 us`.
- [x] [patch] Store initialized page indices in `Page::page_index`, route `index_in_segment` and `page_start` through the stored value, and add const invariants proving `PAGES_PER_SEGMENT` and `NUM_SIZE_CLASSES` fit their metadata fields.
- [x] [patch] Update standalone core page tests to initialize real `Segment` metadata before using `Page::page_start`, preserving the production page-index invariant in tests.
- [x] [patch] Replace allocation hot-path `set_alloc_count(page.alloc_count + 1)` calls with `increment_alloc_count()` in local and heap allocation paths.
- [x] [patch] Regenerate `benchmarks/allocator_comparison.md`; current summary reports saturated threaded small cycles at Mnemosyne `66.851 us` versus mimalloc `70.088 us`, while public small cycle remains `3.018 ns` versus mimalloc `2.724 ns`.
- [x] [patch] Narrow local-free defrag accounting to `became_empty` transitions so full-page-to-active deallocations do not pay periodic-sweep cadence; focused Criterion improved `allocator deallocation latency/Mnemosyne/large_8192` from about `70.679 ns` in the retained table to `29.550 ns`.
- [x] [patch] Reject a guard-free full-page-to-active local-free split after it failed to improve `allocator deallocation latency/Mnemosyne/large_8192` and regressed small/medium/large cycle rows.
- [x] [patch] Reject deferred empty-page migration after focused benchmarking showed no material `large_8192` deallocation improvement.
- [x] [patch] Replace bare heap integration test layout and worker-join unwraps with contextual diagnostics.
- [x] [patch] Replace bare heap unit-test layout unwraps with a shared contextual layout helper.
- [x] [patch] Replace bare branded-vector transition push unwraps with operation-specific diagnostics.
- [x] [patch] Replace bare branded-cell test allocation and vector-push unwraps with operation-specific diagnostics.
- [x] [patch] Replace bare branded container trait-operation unwraps with operation-specific diagnostics.
- [x] [patch] Replace remaining branded vector shrink and extension unwraps with operation-specific diagnostics.
- [x] [patch] Replace the local allocator page-saturation test panic with a value assertion carrying segment/page diagnostics.
- [x] [patch] Replace benchmark utility unwraps with explicit full-page handling and contextual layout diagnostics.
- [x] [patch] Replace remaining Rustdoc example unwraps with contextual allocation diagnostics.
- [x] [patch] Include SnMalloc `huge_2m` benchmark rows in allocator comparisons instead of hard-coded `N/A` omissions.
- [x] [patch] Relax profiler OS TLS-key hot reads and one-time publication CAS to `Ordering::Relaxed`, matching the allocator TLS-key invariant.
