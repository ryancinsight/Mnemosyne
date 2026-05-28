# Checklist

Target version: 0.1.0

## Verified

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

## Open

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

- [ ] [patch] Audit benchmark baseline metadata after bounded Criterion harness configuration.

- [x] [patch] Add compile-time `const _: () = assert!(...)` invariants in `mnemosyne-core::constants` pinning `SEGMENT_SIZE`/`PAGE_SIZE` power-of-two, `SEGMENT_ALIGN == SEGMENT_SIZE`, `PAGE_ALIGN == PAGE_SIZE`, `PAGES_PER_SEGMENT * PAGE_SIZE == SEGMENT_SIZE`, `PAGES_PER_SEGMENT >= 2`, `MAX_SMALL_ALLOC_SIZE <= PAGE_SIZE`, `MAX_ALLOC_SIZE >= SEGMENT_SIZE`, and `NUM_SIZE_CLASSES > 0`.
- [x] [patch] Add compile-time cross-checks in `mnemosyne-core::size_class` that `class_to_size(NUM_SIZE_CLASSES - 1) == MAX_SMALL_ALLOC_SIZE` and `class_to_size(NUM_SIZE_CLASSES) == 0`.
