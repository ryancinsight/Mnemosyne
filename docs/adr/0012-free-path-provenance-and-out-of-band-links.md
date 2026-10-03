# ADR 0012: Reach free-path state without the caller's provenance

Status: Proposed

Revised 2026-10-03: corrected the root cause (wildcard provenance), added the
split write and quarantine options, fixed thresholds before measuring, and
recorded the option-1 prototype's measurements against them.

## Context

`GlobalAlloc::dealloc` receives the caller's pointer, whose Miri tag can be
narrower than the block: a `Box<i32>` is retagged over 4 bytes of a 16-byte
class-0 block. Every class is a multiple of 16 bytes (`CLASS_TO_SIZE`,
`MIN_BLOCK_SIZE`), so the 8-byte link fits every block, but a 1–7 byte request
does not cover it. The free path touches four regions beyond the caller's
bytes: segment and page metadata (`locate_segment`); the link
`Block::next_encoded` (`commit_in_place_free`, `do_local_free_internal_policy`,
`AtomicFreeList::push_dynamic`); the huge back-pointer (`ptr.sub(1)` in
`free_large_or_huge_raw`); and the poison fill and `HardenedPolicy` canary.

**Root cause.** `set_next_dynamic(&mut self)` forms an 8-byte `&mut Block` from
the caller's pointer; when the tag covers fewer than 8 bytes, Stacked Borrows
(SB) rejects the retag. std frees its own argument box while that box's
protector is active (`ThreadInit::init(self: Box<Self>)`), so under Tree
Borrows (TB) a write to those bytes through a tag that is not a child of the
box's tag is UB. The previous revision's "no tag satisfies both models" was an
artifact of PR #210's `with_exposed_provenance_mut`, whose wildcard matches any
exposed tag under SB but is foreign to the protected box under TB. Writing the
covered bytes through the caller's pointer and the rest through a
mapping-derived pointer satisfies both models. Reusing the block before the
protected function returns is UB under every design and is a re-open trigger.

**Upstream.** rust-lang/rust#163403 (closed 2026-09-27: the widening-allocator
problem; TB accepts it, a fix is under way); rust-lang/miri#2104 (open, the
same SB symptom); rust-lang/miri#2686 (open, fresh provenance for
`alloc`/`dealloc`, not implemented in Miri); rust-lang/unsafe-code-guidelines#442.

## Options

Regions 1 and 3 must not use the caller's provenance, and ADR 0009 rules out
exposure, so every conforming option needs a **registry**: each 2 MiB chunk
maps to a pointer carrying its mapping's provenance, registered at
`B::allocate` and cleared at `B::deallocate`. `locate_segment` returns
`donor.with_addr(masked)`; free, `realloc`, `usable_size` and the heap free
consult it.

| Option | Link | SB | TB | Strict | Security |
|---|---|---|---|---|---|
| 1. Registry + split write | in-band | pass (measured) | pass (measured) | pass (measured) | unchanged |
| 2. Registry + quarantine | in-band, caller's ptr | excluded | expected | expected | unchanged |
| 3. Registry + bitmap | out-of-band | expected | expected | expected | weaker |
| 4. Registry + `u16` index array | out-of-band | expected | expected | expected | unchanged |
| 5. Exposed provenance (#210) | either | wildcard | wildcard | unsupported | unchanged |

- **1. Split write.** Link and canary bytes below the request go through the
  caller's pointer, the rest through the mapping-derived pointer, as byte
  copies; no `&mut Block` is formed from the caller's pointer. Free lists and
  the per-CPU cache hold only mapping-derived pointers. Poisoning stops at the
  request. The layout-free paths (`thread_free`, the heap free, C `free`) treat
  the whole block as covered.
- **2. Quarantine.** Links go through the caller's pointer; CI runs TB and
  drops SB for the allocator crates until miri#2686 lands.
- **3. Bitmap.** First-free-by-address allocation defeats the randomized
  `secondary_free` list and drops free-list encryption (ADR 0001). Rejected.
- **4. Index array.** 12.5% of class-0 page capacity, and poisoning and the
  canary still need a split rule. Rejected.
- **5. Exposure.** Violates ADR 0009; strict provenance cannot run it. Rejected.

**Geometry.** A static root indexed by the high chunk bits, and leaves
allocated from `B` on first use, zero-filled, never freed. `VA_BITS` is 47 on
x86-64 (Windows 8.1+, Linux unhinted; Linux `arch/x86/x86_64/5level-paging`),
48 on other 64-bit targets (AArch64 Linux unhinted, `arch/arm64/memory`), and
32 on 32-bit targets; registration refuses a mapping above it. At 47 bits the
root is 64 KiB of `.bss` and each leaf 64 KiB covering 16 GiB. A flat pagemap
(one load per lookup) needs a 512 MiB reservation, which neither Miri nor
wasm32 provides; rejected for now.

## Acceptance thresholds (fixed before measurement)

Option 1 is chosen over option 2 only if every bound holds, measured against
`origin/main` for `StandardPolicy` and `SecurePolicy`:

- **Static path length** (release x86-64): at most +12 instructions on local
  in-place free, local last-block free and remote push (+10 registry lookup,
  +2 size branch); at most +10 on `realloc` and `usable_size`; +0 on alloc.
- **Dynamic instructions** per alloc+free pair: at most +5% on the criterion
  `allocation`, `cross_thread`, `realloc` and `throughput` suites.
- **Cache misses** on the mimalloc-bench workloads (larson, xmalloc-test,
  cache-scratch, cache-thrash, mstress, rptest, sh6bench, cfrac, espresso):
  L1D at most +0.05 per alloc+free; LLC within run-to-run noise.
- **Memory:** registry at most the root plus 64 KiB per 16 GiB span;
  steady-state RSS at most +1% after 10^8 `latency` cycles.
- **Miri:** `global_alloc_tests` and `mnemosyne-memory-core` clean under SB,
  TB and strict provenance, at least four seeds, with CI's
  `-Zmiri-disable-isolation` and #216's `leak::` exclusion (MN-PROF-MIRI-FRAMES)
  as the only flag and filter.

## Evidence

Prototype: PR #217, branch `fix/mnemosyne-free-path-split-write`, head
`743c3309`, against `origin/main` `58e1aee2`.

**Static path length.** Release x86-64, toolchain 1.97.0, the release profile
without `strip`; instructions on the defined path, every compare and branch
counted, a call counted as one plus its callee.

| Path | main | Option 1 | Delta | Bound |
|---|---|---|---|---|
| Standard in-place free (16 B, owner, `alloc_count > 1`) | 130 | 159 | +29 | +12 |
| Secure in-place free, same path | 140 | 173 | +33 | +12 |
| Standard remote push (owner mismatch, first CAS) | 153 | 195 | +42 | +12 |
| `realloc` within class (10 B to 16 B) | 68 | 88 | +20 | +10 |
| `usable_size`, small | 19 | 35 | +16 | +10 |
| alloc (Standard, Secure) | identical code | | +0 | +0 |

One registry lookup is 15 instructions (three branches, two dependent loads),
and every free path performs one, so the local last-block free (not walked)
also exceeds +12. Option 1's own share is the rest: spills of `FreedBlock`'s
three fields, the covered-size branch, and `thread_realloc` no longer inlining.

**Memory.** The registry meets its bound by construction. Private bytes after
10^8 `latency` cycles, mean of four runs: 7,536,640 to 7,676,928, +140,288 B
(+1.86%) against +1%.

**Miri.** `global_alloc_tests` and `mnemosyne-memory-core` pass under SB, TB and
strict provenance on seeds 1–4 at `743c3309` (28 and 36 tests, with only the
flag and filter above); `mnemosyne-local` and `mnemosyne-heap` pass under SB
and TB, and `mnemosyne-arena` under SB with CI's concurrency filter. The bound
holds.

**Unmeasured.** Dynamic instructions and cache misses have no counter source on
the measuring host: PMC needs admin rights, the WSL image is missing, there is
no valgrind, and the mimalloc-bench workloads need Linux.

## Decision

Pending. Option 1 breaches the static path and RSS bounds, so the rule above
selects option 2. But option 2 performs the same registry lookup on the same
paths and holds the same registry memory; its expected cost is the lookup plus
the block rebuild, which still exceeds +12. The thresholds measure the
registry, which both options carry, so they cannot separate the options.
Choosing requires re-specifying the bounds as either option 1's marginal cost
over option 2, or the registry's cost against its own absolute budget.

## Consequences

The registry replaces PR #210's exposed-provenance entry points under either
option. Under option 1 the free functions and `AtomicFreeList::push_dynamic`
take a `FreedBlock`; under option 2 CI drops SB for the allocator crates.
MN-LOCAL-MIRI-UB stays open until one option lands.

## Overturning evidence

A threshold breach; the protected-reuse scenario becoming reachable from std;
miri#2686 landing fresh allocator provenance, which would make the split write
unnecessary.
