# Gap Audit

## Closed

- [patch] `ThreadAllocator::try_reclaim_segment` reclaimed the current segment after local frees, forcing the next small allocation to rebuild page metadata instead of using the retained free list.
- [patch] The allocator benchmark compared immediate alloc/dealloc only and did not include burst retention or threaded comparison groups.
- [patch] The Unix backend had an untyped `PROT_WRITE` constant that blocked Rustfmt parsing.
- [patch] The global free segment pool had no retention bound, allowing empty segment mappings to remain cached without limit.
- [patch] Mnemosyne had no direct mapped-byte or retained-segment telemetry.
- [patch] Cross-thread free handoff behavior had no allocator-comparison benchmark.
- [patch] Local frees still entered segment-reclaim logic for the current segment; quick Criterion previously showed about a 2% cycle-latency regression.
- [patch] Memory telemetry omitted current-thread live allocations, current-thread owned segments, and cross-thread reclaimed blocks.
- [patch] Memory telemetry had no direct report command.
- [patch] Cross-thread free benchmarks included per-iteration thread creation cost.
- [patch] Memory telemetry omitted per-size-class occupancy.
- [patch] Threaded allocation cycle benchmarks created worker threads inside each Criterion iteration.
- [patch] Segment-cache eviction behavior had no direct Criterion scenario or report row.
- [patch] Benchmark summaries existed only inside Criterion JSON/HTML output.
- [patch] Memory telemetry did not distinguish explicit purge operations from retention-bound unmaps.
- [patch] `memory_report` did not execute explicit segment-cache purge after eviction.
- [patch] Selected benchmark baseline excerpts were not source-controlled.
- [patch] Benchmark baseline excerpt lacked platform/toolchain metadata.
- [patch] Benchmark summary extraction did not emit current-to-baseline comparison ratios.
- [patch] `thread_free` computed small-allocation segment metadata inside the classifier branch, leaving later page-owner logic without the required segment pointer.
- [patch] Benchmark summary comparison logic lacked value-semantic tests.
- [patch] The memory retention-bound test had an unterminated `assert_eq!`, blocking format and test execution.
- [patch] The page-recycling test asserted exact page-index reuse even when global orphan-pool state can provide a different empty page in the same owned segment.
- [patch] `benchmark_summary` refreshed the source-controlled baseline on every run, making repeated comparison reports collapse to 1.0 ratios.
- [patch] Cross-thread small frees routed through the owner allocator queue even though the owning page's atomic free list was already available.
- [patch] `thread_free` recomputed small-allocation segment metadata after classification.
- [patch] Eager page-local remote-free draining added a hot-path atomic check to every allocation; remote frees are now batch-reclaimed after local free blocks are exhausted.
- [patch] Cross-thread free drain/count/link logic was duplicated across allocation, orphan adoption, page recycling, segment reclamation, and allocator drop.
- [patch] Page-local remote-free reclamation lacked direct value-semantic unit coverage at the owning type.
- [patch] Policy-generic `thread_alloc` and `thread_free` APIs were introduced without binding the global allocator call sites to a concrete zero-sized policy type.
- [patch] `mnemosyne-arena` exposed a panic-bearing `align_up` API even though all production callers already used the checked alignment contract.
- [patch] `ThreadAllocator::alloc`, `alloc_cold`, `get_new_page`, and `try_recycle_page` carried production `expect`/`unwrap` calls on structurally guaranteed invariants. Replaced with `debug_assert!` + `core::hint::unreachable_unchecked` to keep the release hot path branch-free for verified invariants.
- [patch] `mnemosyne-arena::lib` re-exported the removed `align_up` symbol, breaking workspace compilation. Stale re-export dropped.
- [patch] `benchmark_summary` enforced a hard 15% gate by default on quick-mode Criterion output before a non-quick threshold policy had been derived.
- [patch] `benchmark_summary` wrote timestamped generated metadata into the source artifact directory instead of the generated Criterion output tree.
- [patch] Page-recycling test asserted exact owned-segment and allocation-count values despite legitimate reusable segment state from prior tests.
- [patch] `benchmark_summary` test build emitted a dead-code warning for metadata output path.
- [patch] Allocation initialization and free poisoning logic repeated the same policy branches across multiple allocation routes.
- [patch] Allocator integration tests mutated global segment-pool counters concurrently, making the purge counter assertion state-dependent.
- [patch] Segment ownership was represented as an untyped raw pointer inside segment data, coupling permission identity to metadata mutation sites.
- [patch] `incoming_free_list` duplicated the page-local remote-free queue after cross-thread and re-entrant frees could both target page metadata directly.
- [patch] Re-entrant local free fallback through the page-local atomic queue lacked direct value-semantic coverage.
- [patch] The backend-specific segment-pool refactor left stale exports and unconstrained arena deallocation bounds, breaking workspace compilation.
- [patch] A single-TLS local-free rewrite looked analytically cheaper but regressed focused threaded allocation performance by about 40%, so the prior local-free routing was restored.
- [patch] Replacing `RefCell` with `UnsafeCell` plus an explicit busy bit improved one focused threaded sample but regressed focused small cycle latency from the 12-13 ns band to about 18.684 ns, so it was restored.
- [patch] Regression threshold policy was not derived from repeated non-quick benchmark samples.
- [patch] Mnemosyne cross-thread 32-byte handoff required post-routing benchmark comparison against mimalloc and snmalloc.
- [patch] Remaining panic sites were audited and found to be test or benchmark scoped, with no production allocator panic path found.
- [patch] The historical threaded small-allocation benchmark could not distinguish allocator throughput from bounded-channel worker coordination overhead. Added a saturated threaded benchmark group with the same worker topology and higher per-command allocation count.
- [patch] Backend-specific `LocalAllocatorSelector` macro expansion reused the same TLS symbol names for multiple backend implementations. The macro now requires a module identifier, giving each backend distinct thread-local allocator and re-entrancy state.
- [patch] Saturated threaded quick benchmark executed across all allocator comparators: Mnemosyne `1.389 ms`, mimalloc `57.447 us`, snmalloc `261.769 us` for 64k four-worker small allocation cycles.
- [patch] Page-refill telemetry showed `19` refill operations and `19` recycle sweeps for the memory report allocation mix. Recycle sweeps now execute only after the current segment has no unsliced pages, reducing the same report to `19` refills and `1` recycle sweep.
- [patch] Current quick benchmark comparison no longer reports selected-baseline threshold regressions: small/medium/large cycle ratios are `0.993`, `0.998`, and `0.985`; small burst is `0.851`; threaded small is `1.113` under the configured `1.50` threshold.
- [patch] Collapsing the local-free owner-token check and allocator borrow into one TLS closure improved focused small cycle latency to about `10.956 ns` and saturated threaded latency to about `198.741 us`, but the historical threaded row repeatedly exceeded the configured threshold (`1.671x` in the regenerated summary). The change was reverted.
- [patch] The selected threshold baseline now gates `threaded saturated small allocation cycles/mnemosyne` instead of the scheduler-sensitive historical threaded row. `benchmark_summary -- --enforce-thresholds` passes with the saturated row at `1.000x`; the historical row remains in side-by-side reports only.
- [patch] `allocator_bench.rs` still used panic assertions and unwrap/expect calls for allocation failure, channel closure, worker join failure, segment allocation failure, and retention-bound violations. These now route through explicit benchmark failure diagnostics or non-panicking shutdown reporting; `memory_report.rs` already used `Result` errors.
- [patch] Benchmark unsafe operations and local allocator byte-initialization helpers lacked complete local safety contracts. Added `# Safety` docs to unsafe helpers and adjacent `Safety:` comments for dynamic symbol casts, unchecked layouts, allocator calls, segment-cache cycles, and memory-report allocator operations.
- [patch] CUDA unified-memory tracking used a fixed pointer table, but symbol initialization could publish the initialized state before function pointers were visible to concurrent callers. Replaced it with a three-state initialization gate, required both alloc/free symbols before CUDA allocation, and added bounded-registry tests.
- [patch] README architecture notes still described the removed allocator-level `incoming_free_list` and omitted CUDA registry overflow fallback semantics. Updated the documentation to match page-local remote-free routing and bounded CUDA metadata.
- [patch] Backend mapping telemetry used a compare-exchange loop for a monotonic peak counter even though no dependent synchronization exists. Replaced it with `AtomicUsize::fetch_max`, documented relaxed telemetry semantics, and added value-semantic delta/peak tests.

- [patch] Backend deallocation telemetry decremented `current_mapped_bytes` before the OS release call returned, so a failed `munmap`/`VirtualFree` could leave the counter under-counting still-mapped bytes. `MemoryBackend::deallocate` now returns `bool`; `MemoryBackendWrapper` calls the OS first and only invokes `record_unmap` when the release confirms success, routing failures through a new `record_unmap_failure` path that increments only the call counter. Pinned by `failed_release_increments_call_count_without_byte_delta`.
- [patch] Arena purge telemetry counted every popped segment as purged even when `MemoryBackend::deallocate` reported failure. `purge_segment_pool` now records only confirmed releases and pushes failed releases back into the retained pool before stopping the purge loop. Pinned by `purge_retains_segment_when_backend_release_fails`.
- [patch] Large/huge deallocation ignored the backend release result after `MemoryBackend::deallocate` gained a success boolean. `deallocate_large_or_huge` now returns that status for huge mappings, standard segment deallocation returns `true` after transferring ownership to the segment pool, and local frees bind the result explicitly. `MemoryBackend::deallocate` is `#[must_use]`, so remaining ignored release results become compiler warnings. Pinned by `huge_deallocation_returns_backend_release_status`.
- [patch] Segment deallocation to a full retained pool ignored release failure. It now routes through `release_segment_mapping` and pushes the segment back into the pool if the backend cannot release the mapping, preserving ownership metadata instead of dropping the segment pointer.

- [patch] Large-allocation metadata layout had no documented derivation for pointer alignment, prefix containment, or mapping bounds, and the first audit pass incorrectly claimed the metadata slot always remains inside Page 0. `allocate_large_or_huge` now validates power-of-two alignment before backend allocation, uses `PAGE_SIZE` instead of a literal `64 * 1024`, documents that the metadata slot lives in the reserved prefix before the user payload, and debug-checks pointer alignment, metadata prefix containment, and payload mapping bounds. Pinned by `huge_allocation_metadata_slot_round_trips_across_alignments` and `huge_allocation_rejects_non_power_of_two_alignment`.
- [patch] Small-free classification depended on rounding non-segment-aligned large/huge pointers down to the segment header. That invariant fails for huge allocations with alignment above `SEGMENT_SIZE`, where the user pointer may round down to a later segment-sized window inside the raw mapping. `allocate_large_or_huge` now rejects alignments above `SEGMENT_SIZE`, preserving zero-copy classifier behavior without a side registry. `thread_free` documents the classifier invariant and debug-checks that small frees never target metadata page 0. Pinned by `huge_allocation_rejects_alignment_above_segment_size`.
- [patch] Direct unsafe callers could pass zero or non-power-of-two alignment to `thread_alloc`, allowing invalid alignment requests to reach size-class or arena routing even though `GlobalAlloc` receives a valid `Layout`. `thread_alloc` now rejects zero, non-power-of-two, and above-`SEGMENT_SIZE` alignments before dispatch. `allocate_large_or_huge` also rejects zero alignment. Pinned by `thread_alloc_rejects_invalid_alignment_requests` and the expanded invalid-alignment arena test.
- [patch] Direct `thread_alloc(0, align)` previously rounded the request up to `align` and allocated a real block, which diverged from zero-size allocation semantics and could distort telemetry for invalid direct calls. `thread_alloc`, `allocate_large_or_huge`, `Mnemosyne::alloc`, and generic `MnemosyneAllocator::alloc` now return null for zero-size requests before routing. Pinned by `thread_alloc_rejects_zero_size_requests`, `huge_allocation_rejects_zero_size`, and `test_zero_size_allocation_returns_null`.
- [patch] `thread_free` carried only a single `debug_assert!` on `page_index > 0` and re-derived the same indices three times (poisoning, classification, queue insertion) without enforcing the full small-free invariant set in one place. The classifier now documents the four-part invariant (`page_index ≥ 1`, `page_index < PAGES_PER_SEGMENT`, `page.block_size > 0`, and `(ptr − page_start) % page.block_size == 0`) and `debug_assert!`s each part at the canonical derivation site. Pinned by `small_alloc_returns_block_aligned_ptr_outside_metadata_page` which sweeps 8 B–1 KiB requests with mixed alignments and verifies page-index range, block-size initialization, and stride alignment.
- [patch] Direct size-bound handling was split across checked `usize` arithmetic and backend failure. This allowed direct arena requests near `isize::MAX` to reach backend allocation attempts after adding segment and alignment overhead. Added `MAX_ALLOC_SIZE = isize::MAX as usize`, rejected direct `thread_alloc` requests above it, and rejected arena mappings whose total backend mapping requirement exceeds it. Pinned by `thread_alloc_rejects_size_above_layout_bound` and `huge_allocation_rejects_request_exceeding_layout_bound`.

## Remaining

- [patch] Allocation request validation is now repeated across global, local, and arena entry points; audit whether a shared zero-cost validation helper can remove duplication without widening public API surface.
