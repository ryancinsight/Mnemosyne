#!/usr/bin/env python3
"""Ratchet: every production `unsafe {}` block is preceded by a `// SAFETY:` comment.

Scans the workspace's library and binary sources — test modules (`#[cfg(test)]`
regions and `tests.rs` files), `tests/`, `benches/`, the `fuzz/` crate and the
benchmark harness crate are outside the production surface — and reports each
`unsafe {` whose preceding fourteen lines carry no `// SAFETY:` comment (the
one sanctioned spelling; `// Safety:` was normalized away). The count only
decreases: `check` fails when it exceeds `BASELINE`, and a pass that lowers it
lowers `BASELINE` in the same change (MNEM-UNSAFE-DOC-1).

    python scripts/safety_comment_scan.py          # list the sites
    python scripts/safety_comment_scan.py check    # exit 1 above the baseline
"""
from __future__ import annotations

import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES = ROOT / "crates"
WINDOW = 14
BASELINE = 60  # only ever edited downward
EXCLUDED_CRATES = {"mnemosyne-benchmarks"}
UNSAFE_OPEN = re.compile(r"\bunsafe\s*\{")
CFG_TEST = re.compile(r"#\[cfg\(test\)\]")


def is_production(path: Path) -> bool:
    rel = path.relative_to(CRATES).as_posix()
    crate = rel.split("/", 1)[0]
    if crate in EXCLUDED_CRATES:
        return False
    parts = rel.split("/")
    return not (
        "tests" in parts or "benches" in parts or rel.endswith("/tests.rs")
    )


def undocumented(path: Path) -> list[int]:
    lines = path.read_text(encoding="utf-8", errors="replace").split("\n")
    sites: list[int] = []
    in_test_module = False
    for index, line in enumerate(lines):
        # An inline `#[cfg(test)] mod` sits at the end of its file by
        # convention here, so everything after the attribute is test code.
        if CFG_TEST.search(line):
            in_test_module = True
        if in_test_module or not UNSAFE_OPEN.search(line):
            continue
        window = "\n".join(lines[max(0, index - WINDOW) : index + 1])
        if "// SAFETY:" not in window:
            sites.append(index + 1)
    return sites


def scan() -> list[tuple[str, int]]:
    found: list[tuple[str, int]] = []
    for path in sorted(CRATES.rglob("*.rs")):
        if not is_production(path):
            continue
        rel = path.relative_to(ROOT).as_posix()
        found.extend((rel, line) for line in undocumented(path))
    return found


def main(argv: list[str]) -> int:
    sites = scan()
    by_file = Counter(rel for rel, _ in sites)
    for rel, count in by_file.most_common():
        lines = ", ".join(str(line) for r, line in sites if r == rel)
        print(f"{count:3d} {rel}: {lines}")
    print(f"undocumented unsafe blocks: {len(sites)} (baseline {BASELINE})")
    if argv[1:] == ["check"] and len(sites) > BASELINE:
        print("error: the count rose above the committed baseline", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
