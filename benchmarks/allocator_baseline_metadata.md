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
The variance report at `target/criterion/benchmark_variance.csv` records Criterion mean confidence intervals, relative CI width, and an `unstable` flag. Threaded and cross-thread rows use a `0.25` relative-width threshold because scheduler variance is part of the measured topology; other rows use `0.15`.
The summary command does not mutate the source-controlled baseline unless `--refresh-baseline` is provided.
Default summary runs report threshold ratios without failing the command. Threshold enforcement is explicit with `--enforce-thresholds`; the selected gate currently applies per-row thresholds to small/medium/large Mnemosyne cycle latency, small burst retention, small cross-thread handoff, saturated threaded cycles, and segment cache eviction.
The `Threaded saturated small allocation cycles` group replaces the historical threaded row in the source-controlled baseline excerpt. It isolates allocator throughput from bounded-channel worker coordination by increasing per-command allocation work while preserving the same allocator set and worker topology. The current generated bounded smoke sample measured Mnemosyne at `76.682 us`, mimalloc at `61.718 us`, and snmalloc at `262.230 us` for 64k four-worker small allocation cycles.
The historical `Threaded small allocation cycles` row remains in the side-by-side report for continuity, but it is not a threshold-gated baseline row because per-sample bounded-channel scheduling variance can dominate allocator changes.
The memory report includes page-reset, guard-install, retained-pool reset, page-refill, recycle, fresh-page, fresh-segment, orphan-adoption, and recycle-sweep counters. After recycle-sweep deferral, the report allocation mix measured `19` page refills and `1` recycle sweep.
The current usable-size comparison measured Mnemosyne at `5.586 ns` for 32-byte cycles and `5.697 ns` for 1024-byte cycles on this Windows GNU target.
The current realloc comparison measured Mnemosyne at `5.523 ns` for within-class `24 -> 32` cycles and `12.932 ns` for cross-class `32 -> 64` cycles on this Windows GNU target.
The current isolated usable-size query comparison measured Mnemosyne at `0.411 ns` for 32-byte pointers and `0.383 ns` for 1024-byte pointers on this Windows GNU target.
The current allocation-only comparison measured Mnemosyne at `12.430 ns` for 32-byte allocations and `27.116 ns` for 1024-byte allocations on this Windows GNU target, versus System at `30.536 ns` and `54.648 ns`, mimalloc at `17.426 ns` and `285.593 ns`, and snmalloc at `14.605 ns` and `78.944 ns`.
The current deallocation-only comparison measured Mnemosyne at `6.414 ns` for 32-byte frees and `29.820 ns` for 1024-byte frees on this Windows GNU target, versus System at `20.864 ns` and `92.887 ns`, mimalloc at `5.828 ns` and `114.297 ns`, and snmalloc at `17.283 ns` and `71.771 ns`.
The current selected mimalloc-regression refresh measured Mnemosyne at `8.162 us` for threaded small allocation cycles, `76.682 us` for threaded saturated small allocation cycles, `5.586 ns` for `usable size latency/small_32`, `5.523 ns` for `realloc latency/within_class_24_to_32`, and `12.932 ns` for `realloc latency/cross_class_32_to_64`. The refreshed variance report marks these Mnemosyne rows stable under their row-specific CI-width thresholds.
