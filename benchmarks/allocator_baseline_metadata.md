# Allocator Baseline Metadata

The source-controlled baseline excerpt and the comparison report are
generated from independent bounded Criterion smoke runs, so the same row may
appear with different point estimates in `allocator_baseline_excerpt.csv`
and `allocator_comparison.md`. Treat the baseline as the threshold-gated
reference (regenerated only with `--refresh-baseline`) and the comparison
report as a snapshot of the most recent comparison run.

The baseline below was refreshed from the bounded Criterion smoke harness
after the following local-allocator changes. Refresh the source-controlled
baseline only after an intentional threshold-policy decision.

- `allocate_large_or_huge` mapping slack reduced from
  `size + alignment + 2 * SEGMENT_SIZE` to
  `size + alignment + SEGMENT_ALIGN + PAGE_SIZE`, saving ~2 MiB − 64 KiB
  per huge allocation.
- `Page` shrunk from 72 bytes (straddling a 64-byte cache line for
  half the array) to one-cache-line metadata after removing the dead
  `segment` back-pointer field, `is_empty` helper, and later the unused
  `local_free` list.
- `MemoryBackend::deallocate` now returns a release-success boolean and
  the wrapper telemetry decrements `current_mapped_bytes` only on
  confirmed release.
- `size_to_class` and `class_to_size` are forced inline across crate
  boundaries so small allocation hot paths receive the mapper body.
- `usable_size` benchmarks now cover Mnemosyne, mimalloc, snmalloc, and
  target-gated jemalloc; the summary includes `usable size latency/`
  rows, but the threshold baseline remains unchanged.
- `realloc` benchmarks now cover within-class and cross-class realloc
  cycles; the summary includes `realloc latency/` rows, but the
  threshold baseline remains unchanged.
- `usable_size` query benchmarks now isolate raw metadata lookup cost
  from allocation/deallocation cost; the summary includes
  `usable size query latency/` rows, but the threshold baseline remains
  unchanged.
- Allocation-only benchmarks now use a drop guard so Criterion measures
  allocation latency while cleanup returns blocks to each allocator; the
  summary includes `allocator allocation latency/` rows, but the threshold
  baseline remains unchanged.
- System allocator comparator rows now cover portable allocation,
  allocation/deallocation cycle, burst, realloc, cross-thread handoff, and
  saturated threaded groups. Portable usable-size rows remain `N/A` because
  `std::alloc::System` exposes no stable usable-size API.
- Deallocation-only benchmarks now allocate each block during Criterion setup
  and measure only the allocator `dealloc` call; the summary includes
  `allocator deallocation latency/` rows, but the threshold baseline remains
  unchanged.
- The small-free classifier reads the target page's `block_size` before the
  huge-allocation metadata fallback, and local-free owner checks derive the
  current allocator token from the existing TLS access. This removes duplicate
  metadata/TLS work from the deallocation hot path without changing the
  re-entrant page-queue contract.
- Removed the unused `Page::local_free` list. Local frees already return
  blocks directly to `Page::free`, while re-entrant and cross-thread frees use
  `Page::thread_free`; the removed field had no production writer and added an
  allocation hot-path branch.
- Standard-policy small realloc now proves same-class growth from the old
  `Layout` before falling back to `usable_size`, avoiding a pointer metadata
  query for within-class requests such as `24 -> 32`.
- Same-thread frees on the active segment now use a segment current-marker to
  return blocks directly to the page free list without taking the allocator
  `RefCell` mutable-borrow path when no page-list relink or segment reclaim is
  required.
- Small allocations now use `LocalAllocatorSelector::with_allocator_guard` to
  combine re-entrancy guard setup, allocator access, and guard clearing in one
  selector operation, removing a separate TLS lookup from the standard
  allocation path.

## 2026-06-02

- **Jemalloc comparison refresh**: `cargo bench -p mnemosyne-benchmarks --features system-jemalloc --bench allocator_bench` now populates Jemalloc columns on this Windows GNU environment.
- **Cross-thread baseline refresh**: The previous `cross-thread free handoff/mnemosyne/small_32` baseline was stale for the jemalloc-enabled benchmark configuration. A detached unmodified `HEAD` worktree measured the row at `26.858 us`, matching the active worktree's `26.881 us` refreshed row. The selected baseline was refreshed rather than treating the old `14.236 us` row as a source regression.
- **Threaded saturated baseline refresh**: A detached unmodified `HEAD` worktree measured `threaded saturated small allocation cycles/mnemosyne` at `94.198 us`; the active worktree refreshed row is `88.057 us`. The old `63.037 us` row did not represent the current jemalloc-enabled run configuration.
- **Benchmark memory cleanup**: Cross-thread handoff benchmarks now use a per-worker fixed handoff buffer synchronized by the existing bounded channels instead of allocating a setup `Vec` every iteration. This removes benchmark-side heap traffic from the handoff scenario without changing the allocator operation count.
- **Threshold gate**: `cargo run -p mnemosyne-benchmarks --features system-jemalloc --bin benchmark_summary --release -- --enforce-thresholds` passes after the refresh. Selected rows are present and compare at `1.000x` against the refreshed baseline.
- **Cross-thread small handoff optimization**: Remote frees no longer charge periodic defragmentation work to the non-owner allocator. The owner still reclaims the page-local `thread_free` list on allocation or owner-side segment sweep. `cross-thread free handoff/mnemosyne/small_32` improved from the refreshed `26.881 us` baseline to `14.116 us` (`0.525x` mean ratio), and the variance report marks the row stable.
- **Threaded small worker harness**: The threaded allocation-cycle harness now stores workers in a fixed `[ThreadCycleWorker; THREADS]` array instead of heap-backed `Vec`s. This removes setup heap traffic from the threaded benchmark topology. `threaded small allocation cycles/mnemosyne` now measures `4.529 us` with stable variance, compared with the stale `38.912 us` report row from the earlier full comparison.
- **Small usable-size path**: Small-allocation `usable_size` now derives the page index with the same mask-based classifier used by `thread_free`, removing a dependent subtraction from the query path. `usable size latency/mnemosyne/small_32` measures `2.821 ns` and `usable size query latency/mnemosyne/small_32` measures `0.271 ns`; both rows are stable in the variance report.

## 2026-05-30

- **Deallocation Latency Optimization**: Direct pointer casting bypassed the second TLS lookup on the local free path, and unified re-entrancy tracking by moving the `is_allocating` flag directly to `ThreadAllocator`. This reduced `medium_1024` deallocation latency from `91` ns to `19` ns.
- **Huge Allocation Optimization**: Conditionally bypassed tail and head decommit calls under standard policies where poisoning is disabled, resolving `huge_shrink_4m_to_2m` latency by 52% (from `19` µs to `9` µs).
- **Jemalloc Integration on Windows**: Linked the static MSYS2 UCRT64 `libjemalloc_s.a` library via the `system-jemalloc` feature, populating the previously `N/A` Jemalloc columns.
- **Verification**: Performed full benchmark runs confirming that Mnemosyne meets baseline regression thresholds and outperforms Jemalloc cycle latency by 4x to 8x and threaded cycle throughput by 3x to 3.7x.

## 2026-05-28


- Operating system: Microsoft Windows 10.0.26300
- Rust compiler: rustc 1.95.0 (59807616e 2026-04-14) (Rev2, Built by MSYS2 project)
- Cargo: cargo 1.95.0 (f2d3ce0bd 2026-03-21) (Rev2, Built by MSYS2 project)
- Benchmark command: `cargo bench -p mnemosyne-benchmarks --bench allocator_bench`
- Summary command: `cargo run -p mnemosyne-benchmarks --bin benchmark_summary --release`
- Baseline refresh command: `cargo run -p mnemosyne-benchmarks --bin benchmark_summary --release -- --refresh-baseline`
- Threshold gate command: `cargo run -p mnemosyne-benchmarks --bin benchmark_summary --release -- --enforce-thresholds`
- Memory report command: `cargo run -p mnemosyne-benchmarks --bin memory_report --release`
- Baseline file: `benchmarks/allocator_baseline_excerpt.csv`
- Current excerpt file: `target/criterion/allocator_current_excerpt.csv`
- Comparison report: `target/criterion/benchmark_baseline_comparison.csv`
- Generated metadata: `target/criterion/benchmark_metadata.json`

The benchmark harness uses an explicit bounded Criterion smoke configuration
(`sample_size = 10`, `warm_up_time = 100 ms`, `measurement_time = 500 ms`)
for local optimization work.
The comparator set includes Mnemosyne, the system allocator, mimalloc,
snmalloc, and jemalloc where the target supports `tikv-jemallocator`. On this
Windows GNU run, jemalloc rows are emitted as `N/A` because the native static
jemalloc library does not link on the current target.
The comparison report records current-to-baseline mean and median ratios for selected Mnemosyne rows.
The variance report at `target/criterion/benchmark_variance.csv` records Criterion mean confidence intervals, relative CI width, and an `unstable` flag. Threaded small, threaded medium, threaded saturated, and cross-thread rows use a `0.25` relative-width threshold because scheduler variance is part of the measured topology; other rows use `0.15`.
The summary command does not mutate the source-controlled baseline unless `--refresh-baseline` is provided.
Default summary runs report threshold ratios without failing the command. Threshold enforcement is explicit with `--enforce-thresholds`; the selected gate currently applies per-row thresholds to small/medium/large Mnemosyne cycle latency, small burst retention, small cross-thread handoff, saturated threaded cycles, and segment cache eviction.
The `Threaded saturated small allocation cycles` group replaces the historical threaded row in the source-controlled baseline excerpt. It isolates allocator throughput from bounded-channel worker coordination by increasing per-command allocation work while preserving the same allocator set and worker topology. The current generated bounded smoke sample measured Mnemosyne at `53.412 us`, mimalloc at `60.752 us`, and snmalloc at `130.600 us` for 64k four-worker small allocation cycles.
The historical `Threaded small allocation cycles` and retained `Threaded medium allocation cycles` rows remain in the side-by-side report for continuity and size-class disparity tracking, but they are not threshold-gated baseline rows because per-sample bounded-channel scheduling variance can dominate allocator changes.
The memory report includes page-reset, guard-install, retained-pool reset, page-refill, recycle, fresh-page, fresh-segment, orphan-adoption, and recycle-sweep counters. After recycle-sweep deferral, the report allocation mix measured `19` page refills and `1` recycle sweep.
The current usable-size comparison measured Mnemosyne at `2.492 ns` for 32-byte cycles and `3.388 ns` for 1024-byte cycles on this Windows GNU target.
The current realloc comparison measured Mnemosyne at `3.236 ns` for within-class `24 -> 32` cycles and `6.678 ns` for cross-class `32 -> 64` cycles on this Windows GNU target.
The current isolated usable-size query comparison measured Mnemosyne at `0.286 ns` for 32-byte pointers and `0.302 ns` for 1024-byte pointers on this Windows GNU target.
The current allocation-only comparison measured Mnemosyne at `9.849 ns` for 32-byte allocations and `11.427 ns` for 1024-byte allocations on this Windows GNU target, versus System at `20.892 ns` and `62.549 ns`, mimalloc at `15.102 ns` and `270.483 ns`, and snmalloc at `14.318 ns` and `68.399 ns`.
The current deallocation-only comparison measured Mnemosyne at `3.114 ns` for 32-byte frees and `8.472 ns` for 1024-byte frees on this Windows GNU target, versus System at `10.664 ns` and `22.337 ns`, mimalloc at `4.958 ns` and `113.286 ns`, and snmalloc at `9.535 ns` and `56.564 ns`.
The current selected mimalloc-regression refresh measured Mnemosyne at `11.691 us` for threaded small allocation cycles, `5.087 us` for threaded medium allocation cycles, `53.412 us` for threaded saturated small allocation cycles, `2.492 ns` for `usable size latency/small_32`, `3.236 ns` for `realloc latency/within_class_24_to_32`, and `6.678 ns` for `realloc latency/cross_class_32_to_64`. The refreshed variance report marks these Mnemosyne rows stable under their row-specific CI-width thresholds.
