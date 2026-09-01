#!/usr/bin/env python3
"""Run the allocator benchmark suite repeatedly and check that it agrees with itself.

# Why this exists

`benchmarks/allocator_baseline_excerpt.csv` is the reference the regression gate
(`benchmark_summary --enforce-thresholds`) compares against, at per-row ceilings
between 1.05 and 1.25. A gate is only meaningful when the measurement's
run-to-run spread sits *below* the ceiling it trips at; before MN-464 it did
not, so a rerun and a regression were indistinguishable and the baseline could
not be refreshed without baking that noise into the reference.

Two host behaviours caused it, both now handled inside the benchmark process
itself (`benches/allocator/host.rs`): hybrid-core placement and Windows power
throttling. What is left to the procedure is the part a single process cannot
do -- run the suite several times and prove the runs agree.

# What it does

1. Builds the benchmark and summary binaries **before** any timing starts, so no
   compilation competes with a measurement.
2. Runs the suite `--runs` times, each into its own Criterion output root, after
   `--warmup-runs` discarded runs (the first run on a freshly built binary reads
   systematically slower: cold file cache and cold branch predictors).
3. Reports each gated row's spread against that row's own gate ceiling, via
   `benchmark_summary --repeat-spread`, which reads the same `GATE_ROWS` table
   the gate does. Exits nonzero if any gated row disagrees with itself by more
   than the gate would tolerate.

Nothing here changes what a benchmark measures. It selects how many times the
instrument is read and checks that the readings agree.

# Usage

    # from OUTSIDE the Atlas stack, so the overlay cannot rewrite Cargo.lock
    python D:/atlas/repos/mnemosyne/scripts/allocator_measurement.py \
        --workdir ./measurement --runs 3

    # keep the runs for a later baseline refresh
    python .../allocator_measurement.py --workdir ./measurement --keep

Run it on an otherwise idle host: a concurrent build is exactly the disturbance
the procedure exists to exclude.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
BENCH_PACKAGE = "mnemosyne-benchmarks"
BENCH_TARGET = "allocator_bench"
SUMMARY_BIN = "benchmark_summary"


def cargo(args: list[str], *, cwd: Path, target_dir: str | None) -> subprocess.CompletedProcess:
    env = dict(os.environ)
    if target_dir:
        env["CARGO_TARGET_DIR"] = target_dir
    return subprocess.run(
        ["cargo", *args], cwd=cwd, env=env, capture_output=True, text=True, check=False
    )


def built_executable(stdout: str, target_name: str) -> Path | None:
    """Extracts an artifact path from `--message-format=json` build output."""
    for line in stdout.splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get("reason") != "compiler-artifact":
            continue
        if message.get("target", {}).get("name") != target_name:
            continue
        executable = message.get("executable")
        if executable:
            return Path(executable)
    return None


def build(manifest: Path, target_dir: str | None, outside: Path) -> tuple[Path, Path]:
    """Builds both binaries and returns their paths.

    Built before any timing run so compilation never overlaps a measurement.
    """
    print("building benchmark and summary binaries", flush=True)
    bench = cargo(
        [
            "bench", "--manifest-path", str(manifest), "-p", BENCH_PACKAGE,
            "--bench", BENCH_TARGET, "--no-run", "--message-format=json",
        ],
        cwd=outside,
        target_dir=target_dir,
    )
    if bench.returncode != 0:
        raise SystemExit(f"benchmark build failed:\n{bench.stderr}")
    bench_exe = built_executable(bench.stdout, BENCH_TARGET)

    summary = cargo(
        [
            "build", "--manifest-path", str(manifest), "-p", BENCH_PACKAGE,
            "--bin", SUMMARY_BIN, "--release", "--message-format=json",
        ],
        cwd=outside,
        target_dir=target_dir,
    )
    if summary.returncode != 0:
        raise SystemExit(f"summary build failed:\n{summary.stderr}")
    summary_exe = built_executable(summary.stdout, SUMMARY_BIN)

    if bench_exe is None or summary_exe is None:
        raise SystemExit("cargo reported no executable for the benchmark or summary target")
    return bench_exe, summary_exe


def measure(bench_exe: Path, summary_exe: Path, run_dir: Path, label: str) -> Path:
    """Runs the suite once into `run_dir` and returns its Criterion root."""
    criterion_root = run_dir / "target" / "criterion"
    criterion_root.mkdir(parents=True, exist_ok=True)
    (run_dir / "benchmarks").mkdir(parents=True, exist_ok=True)

    env = dict(os.environ)
    env["CRITERION_HOME"] = str(criterion_root)
    print(f"{label}: running suite", flush=True)
    with (run_dir / "bench.log").open("w", encoding="utf-8") as log:
        bench = subprocess.run(
            [str(bench_exe), "--bench"], cwd=run_dir, env=env,
            stdout=log, stderr=subprocess.STDOUT, check=False,
        )
    if bench.returncode != 0:
        raise SystemExit(f"{label}: benchmark exited {bench.returncode}; see {run_dir / 'bench.log'}")

    with (run_dir / "summary.log").open("w", encoding="utf-8") as log:
        summary = subprocess.run(
            [str(summary_exe)], cwd=run_dir, env=env,
            stdout=log, stderr=subprocess.STDOUT, check=False,
        )
    if summary.returncode != 0:
        raise SystemExit(f"{label}: summary exited {summary.returncode}; see {run_dir / 'summary.log'}")
    return criterion_root


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--workdir", type=Path, required=True,
                        help="directory to hold the per-run Criterion output")
    parser.add_argument("--runs", type=int, default=3,
                        help="measured runs to compare (default: 3)")
    parser.add_argument("--warmup-runs", type=int, default=1,
                        help="discarded runs before the measured ones (default: 1)")
    parser.add_argument("--manifest-path", type=Path, default=REPO_ROOT / "Cargo.toml")
    parser.add_argument("--target-dir", default=os.environ.get("CARGO_TARGET_DIR"),
                        help="shared CARGO_TARGET_DIR (default: the environment's)")
    parser.add_argument("--keep", action="store_true",
                        help="keep the run directories instead of removing them")
    args = parser.parse_args()

    if args.runs < 2:
        parser.error("--runs must be at least 2; a spread needs two observations")

    workdir = args.workdir.resolve()
    workdir.mkdir(parents=True, exist_ok=True)
    bench_exe, summary_exe = build(args.manifest_path.resolve(), args.target_dir, workdir)

    for index in range(args.warmup_runs):
        measure(bench_exe, summary_exe, workdir / f"warmup{index + 1}", f"warm-up {index + 1}")

    roots = [
        measure(bench_exe, summary_exe, workdir / f"run{index + 1}", f"run {index + 1}")
        for index in range(args.runs)
    ]

    print(flush=True)
    agreement = subprocess.run(
        [str(summary_exe), "--repeat-spread", *(str(root) for root in roots)],
        cwd=workdir, check=False,
    )
    if not args.keep:
        for index in range(args.warmup_runs):
            shutil.rmtree(workdir / f"warmup{index + 1}", ignore_errors=True)
    if agreement.returncode != 0:
        print(
            "\nThe suite does not agree with itself; the baseline must not be refreshed "
            "from these runs. Check the reported host preparation line in each run's "
            "bench.log before treating this as an allocator result.",
            file=sys.stderr,
        )
    return agreement.returncode


if __name__ == "__main__":
    sys.exit(main())
