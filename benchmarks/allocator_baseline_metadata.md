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
  half the array) to 64 bytes (one page per cache line) after removing
  the dead `segment` back-pointer field and `is_empty` helper.
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
The summary command does not mutate the source-controlled baseline unless `--refresh-baseline` is provided.
Default summary runs report threshold ratios without failing the command. Threshold enforcement is explicit with `--enforce-thresholds`; the selected gate currently applies per-row thresholds to small/medium/large Mnemosyne cycle latency, small burst retention, small cross-thread handoff, saturated threaded cycles, and segment cache eviction.
The `Threaded saturated small allocation cycles` group replaces the historical threaded row in the source-controlled baseline excerpt. It isolates allocator throughput from bounded-channel worker coordination by increasing per-command allocation work while preserving the same allocator set and worker topology. The current generated bounded smoke sample measured Mnemosyne at `201.364 us`, mimalloc at `55.541 us`, and snmalloc at `264.326 us` for 64k four-worker small allocation cycles.
The historical `Threaded small allocation cycles` row remains in the side-by-side report for continuity, but it is not a threshold-gated baseline row because per-sample bounded-channel scheduling variance can dominate allocator changes.
The memory report includes page-reset, guard-install, retained-pool reset, page-refill, recycle, fresh-page, fresh-segment, orphan-adoption, and recycle-sweep counters. After recycle-sweep deferral, the report allocation mix measured `19` page refills and `1` recycle sweep.
The current usable-size comparison measured Mnemosyne at `16.077 ns` for 32-byte cycles and `15.593 ns` for 1024-byte cycles on this Windows GNU target.
The current realloc comparison measured Mnemosyne at `13.831 ns` for within-class `24 -> 32` cycles and `36.456 ns` for cross-class `32 -> 64` cycles on this Windows GNU target.
The current isolated usable-size query comparison measured Mnemosyne at `0.411 ns` for 32-byte pointers and `0.383 ns` for 1024-byte pointers on this Windows GNU target.
The current allocation-only comparison measured Mnemosyne at `18.224 ns` for 32-byte allocations and `24.992 ns` for 1024-byte allocations on this Windows GNU target, versus System at `36.162 ns` and `105.018 ns`.
The current deallocation-only comparison measured Mnemosyne at `6.414 ns` for 32-byte frees and `29.820 ns` for 1024-byte frees on this Windows GNU target, versus System at `20.864 ns` and `92.887 ns`, mimalloc at `5.828 ns` and `114.297 ns`, and snmalloc at `17.283 ns` and `71.771 ns`.
