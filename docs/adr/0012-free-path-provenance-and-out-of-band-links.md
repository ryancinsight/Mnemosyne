# ADR 0012: Reach free-path state without the caller's provenance

Status: Proposed

Revised 2026-10-03: root cause corrected (wildcard provenance); split write and
quarantine options added with bounds fixed before measuring; bounds re-based on
the minimal Tree Borrows-clean tree once main failed Tree Borrows; decided:
quarantine, on four static-bound breaches against that tree.

## Context

`GlobalAlloc::dealloc` receives the caller's pointer, whose Miri tag can be
narrower than the block: a `Box<i32>` is retagged over 4 bytes of a 16-byte
class-0 block. Every class is a multiple of 16 bytes, so the 8-byte link fits
every block, but a 1–7 byte request does not cover it. The free path touches
four regions beyond the caller's bytes: (1) segment and page metadata
(`locate_segment`); (2) the link `Block::next_encoded`; (3) the huge
back-pointer (`ptr.sub(1)` in `free_large_or_huge_raw`); (4) the poison fill
and `HardenedPolicy` canary.

**Root cause.** Forming an 8-byte `&mut Block` from a tag covering fewer bytes
is rejected by Stacked Borrows (SB). std frees its own argument box while that
box's protector is active (`ThreadInit::init(self: Box<Self>)`), so under Tree
Borrows (TB) a write through a tag that is not a child of the box's tag is UB.
PR #210's `with_exposed_provenance_mut` wildcard matches any exposed tag under
SB but is foreign to the protected box under TB. Reusing a block before the
protected function returns is UB under every design and is a re-open trigger.

**Upstream.** rust-lang/rust#163403 (closed 2026-09-27; TB accepts the
widening allocator); rust-lang/miri#2104 (open, the SB symptom);
rust-lang/miri#2686 (open, fresh provenance for `alloc`/`dealloc`);
rust-lang/unsafe-code-guidelines#442.

## Options

Regions 1 and 3 must not use the caller's provenance, and ADR 0009 rules out
exposure, so every conforming option needs a **registry**: each 2 MiB chunk
maps to a pointer carrying its mapping's provenance, registered when the arena
maps and cleared when it unmaps; `locate_segment` returns
`donor.with_addr(masked)`.

| Option | Link | SB | TB | Security |
|---|---|---|---|---|
| 1. Registry + split write | in-band | pass | pass | unchanged |
| 2. Registry + quarantine | in-band, caller's pointer | partly excluded | pass | unchanged |
| 3. Registry + bitmap | out-of-band | expected | expected | weaker |
| 4. Registry + `u16` index array | out-of-band | expected | expected | unchanged |
| 5. Exposed provenance (#210) | either | wildcard | wildcard | unchanged |

- **1. Split write.** Link and canary bytes below the request go through the
  caller's pointer, the rest through the mapping-derived pointer; free lists
  hold only mapping-derived pointers; poisoning stops at the request.
- **2. Quarantine.** Links, canary, poison and `realloc`'s in-place result go
  through the caller's pointer as before; until miri#2686 lands, CI excludes
  from SB the frees narrower than the link and the in-place `realloc` results
  written past the caller's old size.
- **Rejected:** 3 defeats the randomized `secondary_free` list and free-list
  encryption (ADR 0001); 4 costs 12.5% of class-0 page capacity and still needs
  a split rule; 5 violates ADR 0009 and cannot run under strict provenance.

**Geometry.** A static root over the high chunk bits; leaves from `B` on first
use, never freed. `VA_BITS` is 47 on x86-64, 48 on other 64-bit targets, 32 on
32-bit; registration refuses a mapping above it. At 47 bits root and leaf are
64 KiB each, a leaf covering 16 GiB. A flat pagemap needs a 512 MiB
reservation, which neither Miri nor wasm32 provides.

## Bounds and evidence

Static path length: release x86-64, toolchain 1.97.0, release profile without
`strip`; instructions on the defined path, every compare and branch counted, a
call counted as one plus its callee. One registry lookup is 15 instructions
(three branches, two dependent loads). Option 1 is the prototype on PR #217's
head ref; `origin/main` is `58e1aee2`.

**Against main (bounds fixed first; numbers as recorded).** Bounds: +12 on the
free paths, +10 on `realloc` and `usable_size`, +0 on alloc, private bytes
+1%.

| Path | main | Option 1 | Delta | Bound |
|---|---|---|---|---|
| Standard in-place free (16 B, owner, `alloc_count > 1`) | 130 | 159 | +29 | +12 |
| Secure in-place free, same path | 140 | 173 | +33 | +12 |
| Standard remote push (owner mismatch, first CAS) | 153 | 195 | +42 | +12 |
| `realloc` within class (10 B to 16 B) | 68 | 88 | +20 | +10 |
| `usable_size`, small | 19 | 35 | +16 | +10 |
| alloc (Standard, Secure) | identical code | | +0 | +0 |

Private bytes after 10^8 `latency` cycles, mean of four runs: +140,288 B
(+1.86%). Option 1 passes `global_alloc_tests` and `mnemosyne-memory-core`
under SB, TB and strict provenance, seeds 1–4.

**Main fails TB.** `global_alloc_tests` at `58e1aee2` under TB (seeds 1–4, and
strict provenance, seed 1) aborts while libtest drops its options `Vec`:
`thread_free_classified` reads `page.alloc_count` (`free/classified.rs:101`)
through the page pointer masked from the caller's pointer, whose tag the
allocator's own write (`local_alloc/page/allocation.rs:57`) had disabled.
Quarantine therefore needs the registry too, and the bounds above, which
measure the registry, cannot separate the options.

**Re-based on Q (fixed before Q was measured).** Q is `origin/main` plus the
registry for regions 1 and 3, everything else through the caller's pointer.
Option 1 is chosen only if, for `StandardPolicy` and `SecurePolicy`: static
path +2 on the three free paths (the covered-size compare and branch; spills
count), +0 on `usable_size` and alloc, +15 on `realloc` within class; dynamic
instructions +2 per alloc+free pair on the criterion `allocation`,
`cross_thread`, `realloc` and `throughput` suites; L1D and LLC misses on the
mimalloc-bench workloads within Q's run-to-run spread; private bytes within
Q's spread; Miri as option 1.

| Path | Q | Option 1 | Delta | Bound |
|---|---|---|---|---|
| Standard in-place free | 143 | 159 | +16 | +2 |
| Secure in-place free | 153 | 173 | +20 | +2 |
| Standard remote push | 185 (117 + 68) | 195 | +10 | +2 |
| `realloc` within class | 69 | 88 | +19 | +15 |
| `usable_size`, small | 35 | 35 | +0 | +0 |
| alloc (Standard, Secure) | identical code | | +0 | +0 |

Decisions were carried from main to Q by aligning the two disassemblies; the
walker reproduced main's 130 and option 1's 159. Private bytes: Q 7,672,832
(spread 49,152), option 1 7,667,712; holds. Dynamic instructions and cache
misses were not measured (no counter source on the host); the rule below does
not need them.

**Q under Miri** (#218, #219). TB: `global_alloc_tests` seeds 1–4, 28/28 each
with the `leak::` exclusion (MN-PROF-MIRI-FRAMES) and #216's retention guard;
`counting_allocator` 11/11; with strict provenance, seed 1, 27/28, the
failure the test's own integer-to-pointer cast (`basic.rs:85`), which PR #217
rewrites. Core 36/36 and heap 57/57 pass SB and TB; local 92/92 TB and
91/91 SB with the sub-link probe excluded; arena 142/142 SB with CI's
concurrency filter. SB: both facade harnesses abort while nextest lists them, when a `String`
grown in place within its size class writes past its old 56 bytes through the
caller's tag that `realloc` returned; the `mnemosyne-local` probe of a free
narrower than the link fails SB at the link write. Runs on the Windows host;
interpreted for `x86_64-unknown-linux-gnu` as well, the facade TB filter
passes 38/38 (excluding `policy::test_cuda_backends`, whose `dlopen` Miri does
not support), local passes SB 91/91 and TB 92/92, and facade SB aborts alike.

## Decision

Quarantine (option 2). Option 1 breaches four static bounds against Q, so the
pre-registered rule decides without the counter-gated bounds. The metadata
reads go through the registry (#218, #219); CI runs the facade harnesses under
TB (#216) and excludes from SB the facade's `Mnemosyne`-backed harnesses and
the sub-link `mnemosyne-local` probe. Option 1's prototype stays on PR #217's
head ref.

## Consequences

The registry replaces PR #210's exposed-provenance entry points, and #210
closes. A free through a pointer narrower than the 8-byte link, and a write
past the old size through an in-place `realloc` result, are outside SB
coverage. Every free pays one 15-instruction lookup; the registry holds the
leaves it maps (64 KiB per 16 GiB span).

## Overturning evidence

rust-lang/miri#2686 landing fresh allocator provenance: the SB exclusions
lift, and option 1 is re-measured against Q under the re-based bounds. The
protected-reuse scenario becoming reachable from std.
