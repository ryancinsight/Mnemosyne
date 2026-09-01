# Allocator Baseline Metadata

## 2026-09-01 — pinned measurement procedure (MN-464); baseline still NOT refreshed

The section below this one recorded that the suite disagreed with itself by more
than the gate it feeds: a **12.6% median spread** across three identical runs,
against per-row ceilings of 1.05 to 1.25. This section records what caused that,
what the procedure now does about it, and why the baseline is *still* not
refreshed.

### The measurement procedure

Run it from **outside** the Atlas stack, on an otherwise idle host:

```sh
python D:/atlas/repos/mnemosyne/scripts/allocator_measurement.py \
    --workdir ./measurement --runs 3
```

It builds both binaries before any timing starts, discards one warm-up run, runs
the suite three times into separate Criterion roots, and checks agreement with
`benchmark_summary --repeat-spread`, which reads the same `GATE_ROWS` table the
regression gate reads. A gated row may not move between identical runs by more
than the gate would call a regression; the check exits nonzero when one does.

Two of the three levers live inside the benchmark process, so an ordinary
`cargo bench` gets them too (`benches/allocator/host.rs`,
`benches/allocator/measurement.rs`). Each run's `bench.log` opens with what the
process actually achieved, e.g.

```text
allocator_bench: power throttling opted out; bound to 8 performance cores (mask 0xc03c03)
```

Numbers taken under a `REFUSED` line are not comparable with numbers taken under
a prepared one.

### What actually caused the spread

Measured on this host (Windows 11 26340, `1.97.0-x86_64-pc-windows-msvc`, Core
Ultra 9 285K, High performance power plan), three identical runs per arm with a
discarded warm-up run:

| Arm | Gate-row median spread | Gate rows over their own ceiling |
| --- | --- | --- |
| As committed before this change | 12.3% | 5 / 12 |
| Process pinned to logical 0-7 only | 14.1% | 7 / 12 |
| Full procedure | **1.9%** | **1 / 12** |

1. **Hybrid-core placement — confirmed, but the obvious mask is the wrong one.**
   The 285K's eight performance cores are **not** logical 0-7. Selecting the
   highest efficiency class the platform reports — asked of
   `themis::CpuTopology`, which reads
   `GetLogicalProcessorInformationEx(RelationProcessorCore)` on Windows; the
   harness parsed those records itself when this was measured — yields mask
   `0xc03c03` — logical
   {0, 1, 10, 11, 12, 13, 22, 23}. Confirmed by measurement rather than by
   reading the parse: `allocator cycle latency/mnemosyne/small_32` runs at
   3.31-3.38 ns pinned to any subset of that mask (`0x3`, `0xc00`, `0xc00000`)
   and at 4.09 ns pinned to `0xf0`, which the mask excludes — a 24% difference
   between core classes on one row. This is why the second arm above fails:
   `0xff` contains six efficiency cores, so pinning to it constrains placement
   without fixing it.
2. **Windows power throttling — confirmed, and it degrades a whole run.** With
   pinning and a raised sample budget but no opt-out, one run in four was
   uniformly three to five times slower than its neighbours across nearly every
   row (`realloc latency/mnemosyne/cross_class_32_to_64` 11.4 / 63.5 / 11.3 ns;
   `huge_shrink_4m_to_2m` 9.7 / 28.1 / 9.4 µs) and took 198 s against 148 s for
   the others. That signature — uniform, whole-process, wall-clock visible — is
   `EcoQoS` classifying a long-running non-foreground process as background
   work. `SetProcessInformation(ProcessPowerThrottling, EXECUTION_SPEED)`
   clears it; the technique is Apollo's, from
   `apollo/crates/apollo-fft/benches/engine_census.rs`. Across the four runs of
   the full procedure, elapsed time is 238 / 233 / 244 / 232 s — the spread that
   exposed the throttled run is gone.
3. **Sample budget — confirmed contributory.** The suite ran every row at ten
   samples, 100 ms warm-up, 500 ms measurement. Criterion sizes a row's whole
   iteration count from its warm-up, so one scheduling disturbance inside those
   100 ms mis-sizes every sample that follows, which is how a single row moves
   3-5x while its neighbours do not. The allocator under test now runs at fifty
   samples, 1 s warm-up, 2 s measurement; comparator columns keep the smoke
   budget, because they feed a report rather than a gate.
4. **Ruled out: run ordering alone.** A discarded warm-up run is kept in the
   procedure — the first run on a freshly built binary reads high — but it is
   not the cause. Adding it to the unpinned arm *raised* the measured spread
   from 12.3% to 20.5%, because it changed which disturbances landed in the
   sample, not how many.
5. **Ruled out: cross-row allocator residue as a general effect.** With the
   above applied, 11 of 12 gated rows agree to within 0.3-4.8% while running in
   exactly the same single process, in the same order, with the same
   comparator allocators allocating between them. Residue is not moving the
   suite as a whole. It remains the standing hypothesis for the one row that
   still disagrees.

Secondary evidence: rows the variance report flags `unstable` fall from 37 / 39 /
33 per run to 22 / 26 / 16.

An independent later pair of runs, driven by `scripts/allocator_measurement.py`
rather than by hand, reproduces this: the same eleven rows agree to
**0.09-1.19%**, and the same twelfth breaches at 46.6%.

### Why the baseline is still not refreshed

`realloc latency/mnemosyne/huge_shrink_4m_to_2m` still spreads **30.9%** across
the three runs (12.69 / 11.17 / 9.70 µs), against a 15% ceiling; over all four
runs including the warm-up it is 51.6%. The acceptance oracle for MN-464 is that
*every* gated row agrees within its own ceiling, so it is not met, and a baseline
refreshed now would encode one arbitrary point of that row's distribution as the
reference.

The instrument is no longer the suspect. In the same four runs, the same row
measured for the other allocators is stable:

| Allocator | `huge_shrink_4m_to_2m` across four runs | Spread |
| --- | --- | --- |
| MiMalloc | 274.6 / 276.4 / 270.1 / 269.6 ns | 2.5% |
| System | 964.2 / 948.7 / 928.9 / 949.6 µs | 3.8% |
| RpMalloc | 596.2 / 551.2 / 539.4 / 543.7 µs | 10.5% |
| **Mnemosyne** | **8.37 / 12.69 / 11.17 / 9.70 µs** | **51.6%** |

Three allocators measure that scenario to within 2.5-10.5% on the same host, in
the same runs, under the same procedure. The remaining instability is in
Mnemosyne's 4 MiB→2 MiB huge-realloc path — a real, run-order-dependent
behaviour of the allocator, tracked as its own item rather than absorbed here.

A second reason to hold: **the measurement basis has changed twice over, so the
committed baseline is not comparable to a run of this procedure row for row.**
It was measured under MSYS2 GNU `rustc 1.95.0`; runs are now MSVC `1.97.0`, and
they are additionally pinned to performance cores and unthrottled. The effect is
visible rather than theoretical — `threaded saturated small allocation
cycles/mnemosyne` reads 63.3-63.6 µs under the procedure against 77-85 µs
unpinned, because it now runs on eight performance cores instead of whatever mix
the scheduler chose. When the baseline is refreshed it must be refreshed whole,
under this procedure, and the old numbers discarded rather than compared against.

### Cost

One run of the suite takes 232-244 s on this host, against the 300 s bench-suite
wall-clock bound: about 150 s for the 39 allocator-under-test rows at the gate
budget and about 85 s for the 89 comparator rows at the smoke budget. The
single-iteration CI smoke (`cargo test --benches`) is unaffected at 3.3 s.

## 2026-09-01 — comparison report regenerated; baseline deliberately NOT refreshed

`allocator_comparison.md` had not been regenerated since 2026-07-23
(`63623cb`), 204 commits and the whole MN-437..MN-458 soundness sweep ago. It
is now regenerated. **The threshold baseline
(`allocator_baseline_excerpt.csv`) was not touched**, for the reasons below.

### The recorded toolchain blocker was in one opt-in comparator, not the harness

The blocker on file was that the Criterion executable would not link. Split by
comparator, on this host (Windows 11 26340, `1.97.0-x86_64-pc-windows-msvc`
from `rust-toolchain.toml`):

- **Default feature set builds and runs.** `cargo build -p
  mnemosyne-benchmarks --benches --release` finishes in 42 s. Mnemosyne, the
  system allocator, mimalloc and rpmalloc all produce real rows.
- **`--features snmalloc` cannot build.** `snmalloc-sys` 0.3.8's `build.rs`
  branches on the `MSYSTEM` environment variable *before* it checks
  `is_msvc()`, so under any MSYS2-flavoured shell it hands cmake
  `-DCMAKE_CXX_FLAGS=-fuse-ld=lld -Wno-error=unknown-pragmas` while cmake
  selects the MSVC generator and `cl.exe`. `cl` rejects the GNU flag
  (`command line error D8021: invalid numeric argument
  '/Wno-error=unknown-pragmas'`) and the CXX compiler probe fails. Substituting
  the native CMake 4.3.1 for the MSYS2 one does not help — the flags come from
  the crate, not the generator — and unsetting `MSYSTEM` did not clear them
  either. This is a third-party build-script defect, not a repository one; the
  comparator is opt-in precisely because it also fails on hosted runners.
- **`--features system-jemalloc` cannot link.** This is the genuine
  GNU-vs-MSVC mismatch: the located `D:\msys64\ucrt64\lib\libjemalloc_s.a` is
  a mingw-gcc archive that needs `___chkstk_ms`, a GCC runtime symbol MSVCRT
  does not provide, so `link.exe` reports `LNK2001` on six objects and
  `LNK1120`. The Windows jemalloc column requires either a GNU-hosted rustc or
  an MSVC-built jemalloc; neither exists here.

**Comparators that actually ran: Mnemosyne, System, MiMalloc, RpMalloc.** The
`SnMalloc (ns)` and `Jemalloc (ns)` columns are `N/A` on every row because
those two comparators could not be built or linked, not because they measured
nothing — the diagnoses above are the reason, and `MN-462` / `MN-463` track
them.

### Why the numbers moved, and why it is not the sweep

The stale table was produced under MSYS2 GNU `rustc 1.95.0` with `snmalloc-rs`
still a mandatory dependency; this one under MSVC `rustc 1.97.0` with it
absent. That is a different build, so the two tables are not directly
comparable — several comparator columns move by an order of magnitude
(mimalloc `allocator deallocation latency/huge_2m` goes from `5763.028` ns to
`97.573` ns) purely from the change of C runtime and link set.

To separate the environment from the code, the same benchmark was run against
the pre-sweep revision under the *current* toolchain: `524abee` (2026-08-12,
the parent of MN-437's first commit), with `a80df6f`'s snmalloc opt-in
cherry-picked on so both sides link an identical comparator set. Three runs
each side, comparing medians against measured run-to-run spread:

**No Mnemosyne row regressed beyond noise across the sweep.** Four rows
improved beyond it — `allocator cycle latency/small_32` `0.84x`, `realloc
latency/within_class_24_to_32` `0.90x`, `allocator allocation
latency/large_8192` `0.93x`, `usable size query latency/small_32` `0.93x` —
and everything else sits inside run-to-run variation. The provenance and
segment-ownership rewrites cost nothing measurable.

### Why the baseline was not refreshed

1. **Run-to-run spread on this host swamps the thresholds.** Median spread
   across three consecutive identical runs is **12.6%**, against gate
   thresholds of 5-25%. Individual rows reach 67%
   (`realloc latency/mnemosyne/cross_class_8k_to_16k`: 74.2 / 72.4 / 121.1 ns)
   and the variance report flags 30 rows `unstable`, several with relative CI
   widths above 1.0. A baseline refreshed from one such run would encode noise
   as the reference.
2. **The four "regressions" `--enforce-thresholds` reports against the current
   baseline are all inside that spread** — `allocator cycle
   latency/mnemosyne/small_32` 1.061, `cross-thread free
   handoff/mnemosyne/small_32` 1.641, `realloc
   latency/mnemosyne/within_class_24_to_32` 1.230, `realloc
   latency/mnemosyne/cross_class_8k_to_16k` 1.355 — and the control run
   reproduces them on pre-sweep source, so they are the toolchain change, not
   a code regression.
3. **The existing baseline is itself cross-configuration.** It was measured
   under the GNU toolchain, so the gate currently compares MSVC runs against
   GNU reference values. Refreshing it from a noisy MSVC run would replace one
   mismatch with a noisy reference; the correct fix is a pinned, repeatable
   measurement procedure, tracked as `MN-464`.

### The recorded losses

Of the 14 rows where the stale table showed Mnemosyne behind a comparator,
**13 still show a loss** and one — `threaded small allocation cycles` vs
MiMalloc, `1.51x` — is now a win at a `0.99x` median. Nineteen further ratios
flipped from win to loss, but they are comparator-side: Mnemosyne's own
absolute numbers are unchanged against the pre-sweep control, so those ratios
moved because mimalloc and rpmalloc got faster under the MSVC build, not
because this allocator got slower. `deallocation latency/large_8192` vs
RpMalloc reproduces at `1.84x` (stale `2.25x`); per `gap_audit.md` that avenue
is closed with no safe production optimization identified, and it is recorded
here rather than reopened.

### Reproduction

Run from outside the Atlas stack so the overlay does not rewrite the lockfile:

```sh
export CARGO_TARGET_DIR=D:/atlas/target
export CRITERION_HOME="<scratch>/target/criterion"   # keeps target/ unforked
cd <scratch>                                          # holds ./benchmarks/
cargo bench --manifest-path <repo>/Cargo.toml -p mnemosyne-benchmarks \
    --bench allocator_bench
cargo run  --manifest-path <repo>/Cargo.toml -p mnemosyne-benchmarks \
    --bin benchmark_summary --release
```

`benchmark_summary` resolves `target/criterion` and `benchmarks/` relative to
the working directory, so running it from a scratch directory keeps a stray
run from overwriting the source-controlled baseline even by accident.

---

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
