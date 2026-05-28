# Allocator Baseline Metadata

## 2026-05-27

- Operating system: Microsoft Windows 10.0.26300
- Rust compiler: rustc 1.95.0 (59807616e 2026-04-14) (Rev2, Built by MSYS2 project)
- Cargo: cargo 1.95.0 (f2d3ce0bd 2026-03-21) (Rev2, Built by MSYS2 project)
- Benchmark command: `cargo bench -p mnemosyne-benchmarks --bench allocator_bench -- --quick`
- Summary command: `cargo run -p mnemosyne-benchmarks --bin benchmark_summary --release`
- Baseline refresh command: `cargo run -p mnemosyne-benchmarks --bin benchmark_summary --release -- --refresh-baseline`
- Threshold gate command: `cargo run -p mnemosyne-benchmarks --bin benchmark_summary --release -- --enforce-thresholds`
- Memory report command: `cargo run -p mnemosyne-benchmarks --bin memory_report --release`
- Baseline file: `benchmarks/allocator_baseline_excerpt.csv`
- Current excerpt file: `target/criterion/allocator_current_excerpt.csv`
- Comparison report: `target/criterion/benchmark_baseline_comparison.csv`
- Generated metadata: `target/criterion/benchmark_metadata.json`

The baseline uses Criterion quick mode as a smoke benchmark for local optimization work.
The comparison report records current-to-baseline mean and median ratios for selected Mnemosyne rows.
The summary command does not mutate the source-controlled baseline unless `--refresh-baseline` is provided.
Default summary runs do not fail on threshold ratios. Threshold enforcement is opt-in with `--enforce-thresholds` until the policy is derived from repeated non-quick benchmark samples on the same hardware.
The `Threaded saturated small allocation cycles` group replaces the historical threaded row in the source-controlled baseline excerpt. It isolates allocator throughput from bounded-channel worker coordination by increasing per-command allocation work while preserving the same allocator set and worker topology. The 2026-05-27 quick sample after recycle-sweep deferral measured Mnemosyne at `198.741 us`, mimalloc at `59.046 us`, and snmalloc at `315.518 us` for 64k four-worker small allocation cycles.
The historical `Threaded small allocation cycles` row remains in the side-by-side report for continuity, but it is not a threshold-gated baseline row because per-sample bounded-channel scheduling variance can dominate allocator changes.
The memory report includes page-refill, recycle, fresh-page, fresh-segment, orphan-adoption, and recycle-sweep counters. After recycle-sweep deferral, the report allocation mix measured `19` page refills and `1` recycle sweep.
