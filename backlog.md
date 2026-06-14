# Backlog

## Atlas in-house replacement roadmap — mnemosyne slice [arch]

mnemosyne is the allocation SSOT. The GPU program (coeus/apollo using wgpu + cuda-oxide)
needs a first-class device-memory story beyond the current dlopen `CudaUnifiedBackend`:
- [ ] [arch] Stage D1: device-memory strategy consumed by the `hephaestus` GPU substrate
  (atlas ADR 0001) — device buffer pools, page-locked/pinned host staging, explicit
  unified-vs-discrete policy through the `MemoryBackend` seam. Compose cuda-oxide
  allocation interop with the existing dlopen `cuMemAllocManaged` path; add wgpu
  buffer-pool hooks. ADR.
- [ ] [minor] Stage D1: melinoe-branded device buffers so ownership transfer between
  host/device/stream is a compile-time proof (pairs with hephaestus + coeus Stage D).

### Heterogeneous tiers + kernel resource budgets (atlas ADR 0002)
- [ ] [minor] Tier-keyed device pools: allocation keyed by themis
  `MemoryTier` (`Hbm` vs the new `Gddr`) + `PlacementHint`, with pinned-host
  (`HostPinned`) staging pools, behind the existing `MemoryBackend` seam.
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

## Completed

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
- [patch] Reconcile `complexity_audit.md` with the current free-list/bump-page allocator after the bitmap free-list experiment was rejected.
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

- [patch] Investigate the remaining `allocator deallocation latency/large_8192` gap to RpMalloc. Current retained comparison is Mnemosyne `40.909 ns` versus RpMalloc `6.871 ns` (`5.95x`); the residual work is in same-owner small-page full/active page-list transition cost and benchmark-row variance, not large/huge unmapping.

## Next

- [patch] Reduce the remaining `allocator deallocation latency/large_8192` gap to RpMalloc by isolating owner-validation and page-state transition costs in the same-owner 8 KiB small-page free path without weakening cross-thread free handoff or cycle-latency thresholds.
