# ADR 0012: Reach free-path state without the caller's provenance

Status: Proposed

Revised 2026-10-03: Q pinned and re-measured; narrowed direct frees became a precondition.

## Context

`GlobalAlloc::dealloc` receives the caller's pointer, whose Miri tag can be
narrower than the block: a `Box<i32>` is retagged over 4 bytes of a 16-byte
class-0 block. The free path touches four regions beyond the caller's bytes:
(1) segment and page metadata (`locate_segment`); (2) the free-list link
`Block::next_encoded`; (3) the huge back-pointer before the payload; (4) the
poison fill and `HardenedPolicy` canary.

**Root cause.** Tree Borrows (TB) rejects metadata reads through the masked
caller pointer once the allocator's own writes disabled that tag; Stacked
Borrows (SB) rejects writes past a narrowed tag. std frees its argument box
while its protector is active (`ThreadInit::init(self: Box<Self>)`), so under
TB a write through a non-child tag, such as PR #210's exposed-provenance
wildcard, is UB. Reusing a block before that function returns is UB under
every design and is a re-open trigger.

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
| 2. Registry + quarantine | in-band, caller's pointer | facade: none; `mnemosyne-local`: full | pass | unchanged |
| 3. Registry + bitmap | out-of-band | expected | expected | weaker |
| 4. Registry + `u16` index array | out-of-band | expected | expected | unchanged |
| 5. Exposed provenance (#210) | either | wildcard | wildcard | unchanged |

- **1. Split write.** Link and canary bytes below the request go through the
  caller's pointer, the rest through the mapping-derived pointer; free lists
  hold only mapping-derived pointers; poisoning stops at the request.
- **2. Quarantine.** Links, canary, poison and `realloc`'s in-place result go
  through the caller's pointer; direct frees require it to carry the whole
  block, so std's narrowed pointers reach only the facade.
- **Rejected:** 3 defeats the randomized `secondary_free` list and free-list
  encryption (ADR 0001); 4 costs 12.5% of class-0 page capacity and still needs
  a split rule; 5 violates ADR 0009 and cannot run under strict provenance.

**Geometry.** A static root over the high chunk bits; leaves from the host's
`DefaultBackend` on first use, never freed, so a device backend's teardown
cannot dangle one. `VA_BITS` is 47 on x86-64, 48 on other 64-bit targets, 32
on 32-bit; registration refuses a mapping above it or one holding no chunk
base. At 47 bits root and leaf are 64 KiB each, a leaf covering 16 GiB; a flat
pagemap's 512 MiB reservation is unavailable under Miri and wasm32.

## Bounds and evidence

**Instrument.** `scripts/allocator_path_length.py measure
benchmarks/allocator_path_lengths.json <build>` (PR #223) rebuilds each
count: toolchain 1.97.0, target `x86_64-pc-windows-msvc`, the release profile
without `strip`; every instruction on the path, every compare and branch, a
call counted as one plus its callee; decisions keyed by function offset and
guarded by a fingerprint of the function. One registry lookup is 15
instructions (three branches, two dependent loads). `memory` reports private
bytes (Windows `PrivateUsage`, psutil's `memory_info().private`) after 10^8
alloc/free cycles over the criterion `latency` layouts; four runs per build.

**Builds.** main `58e1aee2`; Q = main plus the registry for regions 1 and 3,
everything else through the caller's pointer, at PR #222's head `7015312c`
(stack #220, #218, #219, #222); option 1 = PR #217's head `743c3309`.

**Bounds, fixed before measuring.** Against main: +12 on the free paths, +10
on `realloc` and `usable_size`, +0 on alloc, private bytes +1%. Option 1 is
chosen only if, against Q, for `StandardPolicy` and `SecurePolicy`: +2 on the
free paths (the covered-size compare and branch; spills count), +0 on
`usable_size` and alloc, +15 on `realloc` within class; dynamic instructions
+2 per alloc+free pair; cache misses and private bytes within Q's spread; Miri
as option 1.

| Path | main | Q | Option 1 | Q − main (bound) | Option 1 − Q (bound) |
|---|---|---|---|---|---|
| Standard in-place free (16 B, owner) | 130 | 145 | 159 | +15 (+12) | +14 (+2) |
| Secure in-place free, same path | 140 | 155 | 173 | +15 (+12) | +18 (+2) |
| Standard remote push (first CAS) | 153 = 102 + 51 | 164 = 121 + 43 | 195 = 125 + 70 | +11 (+12) | not compared |
| `realloc` within class (10 B to 16 B) | 68 | 69 | 88 | +1 (+10) | +19 (+15) |
| `usable_size`, small | 19 | 35 | 35 | +16 (+10) | +0 (+0) |
| alloc (Standard, Secure) | identical code | | | +0 (+0) | +0 (+0) |
| Private bytes, mean of 4 | 7,535,616 | 7,680,000 | 7,669,760 | +1.92% (+1%) | −10,240 (Q spread 24,576) |
| Facade under SB | aborts | aborts | passes | | |

**Remote push.** Before PR #222, Q's remote free located its segment three
times: in `thread_free_classified`, then in the atomic free-list push for the
mode check and, with encrypted links, for the cookie; the push measured 185 =
117 + 68. PR #222 passes the located segment down, so the push repeats no
lookup: 164 = 121 + 43, within the +12 bound. Option 1's prototype predates
that change, so its remote row does not compare options; the in-place rows do.

**Accepted breaches against main.** In-place free +15 against +12 and
`usable_size` +16 against +10 are the one registry lookup each path now needs
(on the free: the lookup's 15, a spilled register's push and pop, two
instructions the compiler dropped). Private bytes +1.92% against +1% are not
attributed further; two 64 KiB leaves would be 131,072 B of the 144,384 B
(hypothesis, not measured). Main fails TB on every program, so a TB-sound free
path pays at least that lookup under this geometry; the bounds stay as written
and these breaches are accepted, not re-based. Dynamic instructions and cache
misses were not measured (no counter source on the host).

**Miri, three deciding facts** (Windows host; `x86_64-unknown-linux-gnu`
interpreted alike):

1. Main fails TB: `global_alloc_tests` aborts at `free/classified.rs:101`
   reading page metadata through the masked caller pointer, and so does
   `provenance::small_blocks_freed_through_narrowed_pointers_are_reused_without_aliasing`.
   Every option needs the registry.
2. Q passes TB on `global_alloc_tests` (at #216's head: default seed and
   seeds 1–4, `leak::` excluded until MN-PROF-MIRI-FRAMES), SB and TB on
   `mnemosyne-local` (no exclusion), core and heap, and SB on arena.
3. Under SB the facade harnesses abort while nextest lists them: a `String`
   grown in place within its size class writes past its old 56 bytes through
   the caller's tag that `realloc` returned. Facade SB coverage is zero.

## Decision

Quarantine (option 2). Option 1 breaches the re-based bounds on both in-place
free paths and on `realloc`. The metadata and huge back-pointer reads go
through the registry (#220, #218, #219, #222). `thread_free` and its direct
siblings require `ptr` to carry provenance over the whole block the allocating
call returned. The facade's `GlobalAlloc::dealloc` and in-place `realloc`
receive std's narrowed pointers: TB accepts their writes, SB rejects them, so
CI runs the facade under TB only (#216) and **facade SB coverage is zero**
until miri#2686. Option 1's prototype stays on PR #217's head ref.

## Consequences

The registry replaces PR #210's exposed-provenance entry points. The former
`mnemosyne-local` SB exclusion became a documented precondition of the direct
frees; its probe is deleted and the local SB step runs unfiltered. Facade SB
coverage is zero: MN-LOCAL-MIRI-UB stays `blocked` on miri#2686, and
`counting_allocator` keeps its `System` inner under Miri. Every free pays one
15-instruction lookup, an accepted breach of the main bounds above; the
registry holds 64 KiB of leaf per 16 GiB span mapped, outside the backend's
mapped-bytes counter (`installed_leaf_bytes`).

## Overturning evidence

rust-lang/miri#2686 landing fresh allocator provenance lifts the facade and
`realloc` quarantine directly: the facade gains an SB step. An option-1 build
meeting the re-based bounds against Q reopens option 1. The protected-reuse
scenario becoming reachable from std.
