# ADR 0012: Reach free-path state without the caller's provenance

Status: Proposed

Revised 2026-10-03: corrected root cause (wildcard provenance), added split
write and quarantine, fixed thresholds before measuring.

## Context

`GlobalAlloc::dealloc` receives the caller's pointer, whose Miri tag can be
narrower than the block: a `Box<i32>` is retagged over 4 bytes of a 16-byte
class-0 block. Every class is a multiple of 16 bytes and at least 16
(`CLASS_TO_SIZE`, `MIN_BLOCK_SIZE`), so the 8-byte link always fits the block,
but a 1–7 byte request does not cover the whole link. The free path touches
four regions beyond the caller's bytes:

1. segment and page metadata, located by `locate_segment` (`map_addr` on the
   caller's pointer, per ADR 0009);
2. the link `Block::next_encoded`, written by `Block::set_next_dynamic` from
   `commit_in_place_free`, `do_local_free_internal_policy` and
   `AtomicFreeList::push_dynamic`;
3. the huge back-pointer, read by `free_large_or_huge_raw` through
   `ptr.sub(1)`;
4. the poison fill and the `HardenedPolicy` canary at `block + 8`.

**Root cause.** Two facts produce the Miri failures:

- `set_next_dynamic(&mut self)` creates an 8-byte `&mut Block` from the
  caller's pointer. When the caller's tag covers fewer than 8 bytes, Stacked
  Borrows (SB) rejects the *retag* ("trying to retag … for SharedReadWrite",
  `basic.rs:8`).
- std frees its own argument box while that box's protector is active
  (`ThreadInit::init(self: Box<Self>)`, std `thread/lifecycle.rs`). Under Tree
  Borrows (TB), a write to those bytes through any tag that is not a child of
  the box's tag is UB.

The previous revision concluded that no tag satisfies both models. That
conclusion was an artifact of PR #210, which rebuilt the block pointer with
`with_exposed_provenance_mut`. The resulting wildcard pointer matches any
exposed tag under SB, but counts as foreign to the protected box under TB.

Two concrete tags do satisfy both models. The covered bytes go through the
caller's pointer, and the rest go through a mapping-derived pointer. The
judge's probe passed this split under SB, TB and `-Zmiri-strict-provenance`
for three scenarios: free then reuse, remote free, and std's protected box.

Reusing the block before the protected function returns is UB under every
design. That case is not solved here and is a re-open trigger.

**Upstream.**

- rust-lang/rust#163403 (closed, 2026-09-27): the Miri maintainer classed
  this as the known widening-allocator problem, noted that TB accepts it, and
  said a fix is under way.
- rust-lang/miri#2104 (open): the same SB symptom.
- rust-lang/miri#2686 (open, "Refresh provenance of global allocator"): fresh
  provenance for `alloc`/`dealloc`. As of 2026-08-24, LLVM is converging on
  an allocator provenance model, and the Miri side is not yet implemented.
- rust-lang/unsafe-code-guidelines#442: the open semantics question.

## Options

Every option must reach paths 1 and 3 without the caller's provenance.

- PR #210 exposes provenance, which violates ADR 0009.
- The conforming alternative is a **registry**. It maps each 2 MiB chunk to a
  pointer that carries its mapping's provenance. The arena registers every
  chunk of a mapping at `B::allocate` and unregisters it at `B::deallocate`.
- `locate_segment` keeps its signature and returns `donor.with_addr(masked)`.
- Every caller passes an allocation-start or block address, so its chunk is
  registered: free, `realloc`, `usable_size`, the heap free, and
  `push_dynamic_with`.

| Option | Link | SB | TB | Strict | Security |
|---|---|---|---|---|---|
| 1. Registry + split write | in-band | expected | expected | expected | unchanged |
| 2. Registry + quarantine | in-band, caller's ptr | excluded | expected | expected | unchanged |
| 3. Registry + bitmap | out-of-band | expected | expected | expected | weaker |
| 4. Registry + `u16` index array | out-of-band | expected | expected | expected | unchanged |
| 5. Exposed provenance (#210) | either | wildcard | wildcard | unsupported | unchanged |

- **1. Split write.**
  - The first `min(size, 8)` bytes of the link go through the caller's
    pointer, and the rest through the mapping-derived block pointer. Both are
    raw byte copies, and no `&mut Block` is formed from the caller's pointer.
  - Heads and the per-CPU cache hold only mapping-derived pointers, so reuse
    never hands out a dead caller tag.
  - `thread_free_layout` supplies the size. Paths without a layout
    (`thread_free`, C `free`) treat the block as covered.
  - Poisoning writes `layout.size()` bytes, or `block_size` bytes on C `free`.
    The canary is split the same way.
- **2. Quarantine.** In-band links go through the caller's pointer. CI runs TB
  and excludes SB from the allocator crates, with miri#2686 as the re-open
  trigger. SB coverage of the free path is lost, while the rest of the stack
  keeps SB.
- **3. Bitmap.** Allocation becomes first-free-by-address within a page. That
  defeats the randomized `Page::secondary_free` list, which
  `RANDOMIZE_ALLOCATION` enables in `SecurePolicy` and `HardenedPolicy`. It
  also removes free-list encryption (ADR 0001). Rejected on security.
- **4. Index array.** Costs 12.5% of page capacity at class 0, and poisoning
  and the canary still need a split rule. Rejected on memory.
- **5. Exposure.** Violates ADR 0009, and 5 of 36 `mnemosyne-memory-core`
  tests become unsupported under strict provenance. Rejected.

**Geometry.** The registry has two levels, with 8192-entry leaves of 64 KiB
that each cover 16 GiB. Leaves come from `B` and are never freed. `VA_BITS` is
a per-target constant. A lookup bounds-checks its level-1 index. Registration
refuses an address it cannot index and fails that mapping rather than masking
the address.

| Target | User VA bits | Level 1 |
|---|---|---|
| x86-64: Windows 8.1+ (128 TB), Linux 4-level | 47 | 64 KiB |
| x86-64: Linux LA57 | 47 unless mmap is hinted above 2^47 | 64 KiB |
| AArch64: Linux 48/52-bit | 48 unless mmap is hinted above 2^48 | 128 KiB |
| wasm32 | 32 | 16 KiB, flat |

Sources: Linux docs `arch/x86/x86_64/5level-paging` §30.3.2 and
`arch/arm64/memory` ("52-bit userspace VAs"). Mnemosyne passes no such hint.

A flat, lazily committed pagemap needs only one load per lookup, but it needs
a 512 MiB reservation at 47 bits, which neither Miri nor wasm32 can provide.
Rejected for now.

## Acceptance thresholds (fixed before measurement)

Option 1 is chosen over option 2 only if every bound below holds. Each is
measured against `origin/main` at the same revision, for `StandardPolicy` and
`SecurePolicy`.

- **Static path length** (release x86-64):
  - at most +12 instructions on local in-place free, local last-block free
    and remote push. This allows +10 for the registry lookup (13 against 3,
    from the first revision) and +2 for the size branch;
  - at most +10 on `realloc` and `usable_size`;
  - +0 on alloc pop.
- **Dynamic instructions** per alloc+free pair: at most +5% on the criterion
  `allocation`, `cross_thread`, `realloc` and `throughput` suites.
- **Cache misses**, measured on larson, xmalloc-test, cache-scratch,
  cache-thrash, mstress, rptest, sh6bench, cfrac and espresso:
  - L1D misses per alloc+free at most +0.05;
  - LLC misses within the run-to-run noise bound.
- **Memory:**
  - registry size at most level 1 plus 64 KiB per 16 GiB span;
  - steady-state RSS at most +1% after 10^8 `latency` churn operations.
- **Miri:** `global_alloc_tests` and `mnemosyne-memory-core` clean under SB,
  TB and strict provenance, with at least four `-Zmiri-seed` values and no
  added flag or filter.

**Instruments.** Static path length comes from release disassembly. This host
has no counter source:

- Core Ultra 9 285K, Windows 11, no admin rights, so wpr/xperf PMC tracing is
  unavailable;
- the WSL disk image is missing, and there is no valgrind;
- wall-clock timing is invalid at the measured 88% load;
- the mimalloc-bench workloads are absent from the repository and need Linux.

The cache, dynamic-instruction and RSS bounds stay unmeasured until one of
these is available: elevated PMC, a repaired WSL, or iai-callgrind on a Linux
host.

## Decision

Pending the evidence. Option 1 is chosen if every threshold holds, option 2
otherwise. Both keep the registry, so ADR 0009 stands.

## Consequences

The registry replaces PR #210's exposed-provenance entry points.

- Under option 1, the free functions and `AtomicFreeList::push_dynamic` take
  the request size, and the per-CPU cache stores mapping-derived pointers.
- Under option 2, CI drops SB for the allocator crates.

MN-LOCAL-MIRI-UB stays open until one option lands.

## Overturning evidence

- A threshold breach.
- The protected-reuse scenario becoming reachable from std.
- miri#2686 landing fresh allocator provenance, which would make the split
  write unnecessary.
