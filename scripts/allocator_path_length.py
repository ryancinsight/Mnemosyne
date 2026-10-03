#!/usr/bin/env python3
"""Static path lengths of the allocator entry points (ADR 0012).

Builds a probe cdylib over a revision exported from git, disassembles it, and
walks each recorded path: every instruction on the path counts, a call counts
as one and its callee is walked as a separate part, and each conditional jump
follows the recorded decision (`<offset>=T|F`, taken or not, keyed by offset
from the function start). A fingerprint of each function guards its decisions,
so a path that no longer matches the code fails instead of miscounting.

Host: Windows with the recorded toolchain and target installed (the probe links
with MSVC `/MAP` to name functions). Revisions that are pull-request heads are
fetched first: `git fetch origin pull/<N>/head`.

`memory` reports the process's private bytes (Windows `PrivateUsage`, the
counter psutil reads as `memory_info().private`) after 10^8 alloc/free cycles
over the criterion `latency` layouts, once per run.

usage:
  allocator_path_length.py measure <paths.json> <build>
  allocator_path_length.py trace <paths.json> <build> <function> [decisions]
  allocator_path_length.py memory <paths.json> <build> [runs]
"""

import bisect
import ctypes
import hashlib
import io
import json
import os
import re
import subprocess
import sys
import tarfile
import tempfile

PROBE = """use core::alloc::{GlobalAlloc, Layout};
use mnemosyne::{Mnemosyne, MnemosyneAllocator, SecurePolicy};

const SECURE: MnemosyneAllocator<SecurePolicy> = MnemosyneAllocator::new();

macro_rules! probes {
    ($dealloc:ident, $alloc:ident, $realloc:ident, $a:expr) => {
        #[unsafe(no_mangle)]
        #[inline(never)]
        pub unsafe extern "C" fn $dealloc(p: *mut u8, size: usize, align: usize) {
            // SAFETY: forwarded from the caller, as for the `GlobalAlloc` method.
            unsafe { $a.dealloc(p, Layout::from_size_align_unchecked(size, align)) }
        }
        #[unsafe(no_mangle)]
        #[inline(never)]
        pub unsafe extern "C" fn $alloc(size: usize, align: usize) -> *mut u8 {
            // SAFETY: forwarded from the caller, as for the `GlobalAlloc` method.
            unsafe { $a.alloc(Layout::from_size_align_unchecked(size, align)) }
        }
        #[unsafe(no_mangle)]
        #[inline(never)]
        pub unsafe extern "C" fn $realloc(p: *mut u8, size: usize, align: usize, new: usize) -> *mut u8 {
            // SAFETY: forwarded from the caller, as for the `GlobalAlloc` method.
            unsafe { $a.realloc(p, Layout::from_size_align_unchecked(size, align), new) }
        }
    };
}
probes!(probe_dealloc, probe_alloc, probe_realloc, Mnemosyne);
probes!(probe_secure_dealloc, probe_secure_alloc, probe_secure_realloc, SECURE);

#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn probe_usable_size(p: *mut u8) -> usize {
    // SAFETY: forwarded from the caller, as for `mnemosyne::usable_size`.
    unsafe { mnemosyne::usable_size(p) }
}
"""

MEMORY_PROBE = """use core::alloc::{GlobalAlloc, Layout};
use core::hint::black_box;
use mnemosyne::Mnemosyne;
use std::io::Read;

#[global_allocator]
static GLOBAL: Mnemosyne = Mnemosyne;

const LAYOUTS: [(usize, usize); 4] = [(32, 8), (1024, 8), (8192, 8), (2 << 20, 4096)];

fn main() {
    for (size, align) in LAYOUTS {
        let layout = Layout::from_size_align(size, align).expect("invariant: valid layout");
        for _ in 0..25_000_000 {
            // SAFETY: `layout` is non-zero-sized; the block is freed with it.
            unsafe {
                let ptr = Mnemosyne.alloc(black_box(layout));
                assert!(!ptr.is_null(), "allocation failed");
                Mnemosyne.dealloc(black_box(ptr), layout);
            }
        }
    }
    println!("done");
    let _read = std::io::stdin().read(&mut [0_u8; 1]);
}
"""

# The workspace release profile except `strip`, so the symbols survive.
MANIFEST = """[package]
name = "probe"
version = "0.0.0"
edition = "2024"
publish = false

{kind}

[dependencies]
mnemosyne = {{ package = "mnemosyne-memory", path = "{tree}/crates/mnemosyne" }}

[profile.release]
debug = false
strip = "none"
lto = "thin"
codegen-units = 1

[workspace]
"""


def compile_probe(spec, rev, work, source, kind, *rustc_args):
    """Builds a probe crate over `rev` exported into `work`; returns its artifact."""
    tree, probe = os.path.join(work, "tree"), os.path.join(work, "probe")
    archive = subprocess.run(["git", "archive", rev], capture_output=True, check=True, timeout=300)
    with tarfile.open(fileobj=io.BytesIO(archive.stdout)) as tar:
        tar.extractall(tree, filter="data")
    os.makedirs(os.path.join(probe, "src"))
    if kind == "lib":
        section = '[lib]\ncrate-type = ["cdylib"]'
    else:
        section = '[[bin]]\nname = "probe"\npath = "src/lib.rs"'
    with open(os.path.join(probe, "Cargo.toml"), "w") as f:
        f.write(MANIFEST.format(kind=section, tree=tree.replace("\\", "/")))
    with open(os.path.join(probe, "src", "lib.rs"), "w") as f:
        f.write(source)
    toolchain, target = spec["toolchain"], spec["target"]
    selector = ["--lib"] if kind == "lib" else ["--bin", "probe"]
    subprocess.run(["cargo", f"+{toolchain}", "rustc", "--release", *selector, "--target", target,
                    "--manifest-path", os.path.join(probe, "Cargo.toml"), "--", *rustc_args],
                   check=True, timeout=1200)
    target_dir = os.environ.get("CARGO_TARGET_DIR", os.path.join(probe, "target"))
    return os.path.join(target_dir, target, "release", "probe.dll" if kind == "lib" else "probe.exe")


def private_bytes(exe):
    """`PrivateUsage` of `exe` once it reports done, its allocator state still live."""
    proc = subprocess.Popen([exe], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    if proc.stdout.readline().strip() != "done":
        sys.exit(f"{exe} did not report done")

    class Counters(ctypes.Structure):
        _fields_ = [("cb", ctypes.c_uint32), ("faults", ctypes.c_uint32)] + [
            (n, ctypes.c_size_t) for n in ("peak_wset", "wset", "peak_paged", "paged",
                                            "peak_nonpaged", "nonpaged", "pagefile",
                                            "peak_pagefile", "private")]

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.OpenProcess.restype = ctypes.c_void_p
    kernel32.K32GetProcessMemoryInfo.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32]
    kernel32.CloseHandle.argtypes = [ctypes.c_void_p]
    handle = kernel32.OpenProcess(0x1000, False, proc.pid)  # PROCESS_QUERY_LIMITED_INFORMATION
    counters = Counters(cb=ctypes.sizeof(Counters))
    ok = handle and kernel32.K32GetProcessMemoryInfo(handle, ctypes.byref(counters), counters.cb)
    if handle:
        kernel32.CloseHandle(handle)
    proc.stdin.close()
    proc.wait(timeout=60)
    if not ok:
        sys.exit(f"GetProcessMemoryInfo failed: error {ctypes.get_last_error()}")
    return counters.private


def build(spec, rev, work):
    """Builds the path probe over `rev`; returns {symbol: (start, [(addr, insn)])}."""
    mapfile = os.path.join(work, "probe.map")
    dll = compile_probe(spec, rev, work, PROBE, "lib", f"-Clink-arg=/MAP:{mapfile}")
    toolchain, target = spec["toolchain"], spec["target"]
    syms = {}
    with open(mapfile, encoding="latin-1") as f:
        for line in f:
            m = re.match(r"\s*[0-9a-f]{4}:[0-9a-f]{8}\s+(\S+)\s+([0-9a-f]{16})\s", line)
            if m:
                syms.setdefault(int(m.group(2), 16), m.group(1))
    starts = sorted(syms)
    objdump = os.path.expanduser(
        f"~/.rustup/toolchains/{toolchain}-{target}/lib/rustlib/{target}/bin/llvm-objdump.exe")
    text = subprocess.run([objdump, "-d", "--no-show-raw-insn", "-M", "intel", dll],
                          capture_output=True, text=True, check=True, timeout=300).stdout
    funcs = {}
    for line in text.splitlines():
        m = re.match(r"\s*([0-9a-f]+):\s+(.*)", line)
        if m:
            addr = int(m.group(1), 16)
            i = bisect.bisect_right(starts, addr) - 1
            if i >= 0:
                funcs.setdefault(syms[starts[i]], (starts[i], []))[1].append((addr, m.group(2)))
    return funcs


def find(funcs, name):
    """The one function whose symbol contains every `&`-separated part of `name`."""
    hits = [s for s in funcs if s == name] or [s for s in funcs if all(p in s for p in name.split("&"))]
    if len(hits) != 1:
        sys.exit(f"{name}: {len(hits)} matching functions")
    return funcs[hits[0]]


def fingerprint(start, insns):
    """Hash of the function's offsets and its instructions with immediates removed."""
    h = hashlib.sha256()
    for addr, text in insns:
        h.update(f"{addr - start:x} {re.sub(r'0x[0-9a-f]+|\b\d+\b', 'N', text)}\n".encode())
    return h.hexdigest()[:16]


def walk(start, insns, decisions, out=None):
    """(instructions on the path, why it stopped early or None)."""
    dec = {int(k, 16): v for k, v in (d.split("=") for d in decisions.split(",") if d)}
    index = {addr: i for i, (addr, _) in enumerate(insns)}
    i = count = 0
    while i < len(insns):
        addr, text = insns[i]
        count += 1
        if out is not None:
            out.append(f"{count:4d} {addr - start:5x} {text[:140]}")
        op = text.split()[0] if text else ""
        tgt = re.search(r"0x([0-9a-f]+)", text)
        dest = int(tgt.group(1), 16) if tgt else None
        if op == "ret" or (op == "jmp" and dest not in index):
            return count, None
        if op.startswith("j") and op != "jmp":
            if addr - start not in dec:
                return count, f"undecided jump at {addr - start:x}"
            if dec[addr - start] == "F":
                i += 1
                continue
            if dest not in index:
                return count, None
        if op.startswith("j"):
            i = index[dest]
            continue
        i += 1
    return count, "fell off the function"


def main():
    cmd, paths_file, name = sys.argv[1:4]
    with open(paths_file) as f:
        spec = json.load(f)
    build_spec = spec["builds"][name]
    with tempfile.TemporaryDirectory(prefix="path-length-") as work:
        if cmd == "memory":
            exe = compile_probe(spec, build_spec["revision"], work, MEMORY_PROBE, "bin")
            for _ in range(int(sys.argv[4]) if len(sys.argv) > 4 else 4):
                print(f"private bytes: {private_bytes(exe)}")
            return
        funcs = build(spec, build_spec["revision"], work)
    if cmd == "trace":
        start, insns = find(funcs, sys.argv[4])
        lines = []
        count, stop = walk(start, insns, sys.argv[5] if len(sys.argv) > 5 else "", lines)
        print("\n".join(lines), f"\nfingerprint {fingerprint(start, insns)} count {count} {stop or ''}")
        return
    failed = False
    for path, parts in build_spec["paths"].items():
        counts = []
        for part in parts:
            start, insns = find(funcs, part["function"])
            if fingerprint(start, insns) != part["fingerprint"]:
                sys.exit(f"{path}: {part['function']} changed since its decisions were recorded")
            count, stop = walk(start, insns, part["decisions"])
            failed |= stop is not None
            counts.append((count, stop))
        detail = " + ".join(f"{c}" + (f" ({s})" if s else "") for c, s in counts)
        print(f"{path}: {sum(c for c, _ in counts)} = {detail}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
