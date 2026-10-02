# ADR 0012: Reach free-path state without the caller's provenance

Status: Proposed

## Context

`GlobalAlloc::dealloc` receives the caller's pointer. Under Miri's aliasing
models that pointer carries the caller's tag, which may be narrower than the
block: a `Box<i32>` is retagged over 4 bytes of a 16-byte class-0 block.
The free path touches three things beyond those bytes:

1. **Segment and page metadata.** These are reached by masking the caller's
   address with `map_addr`, as ADR 0009 requires.
2. **The in-block free-list link.** This is an 8-byte `Block::next_encoded`,
   written at the start of the freed block (`free_helpers.rs:69` for local
   frees, `AtomicFreeList::push_raw_with` for remote frees).
3. **The huge-allocation back-pointer** one slot before the payload.

MN-LOCAL-MIRI-UB recorded Miri UB on paths 1 and 3 under Stacked Borrows (SB)
and Tree Borrows (TB). PR #210 fixed those paths by exposing each mapping and
rebuilding metadata pointers with `with_exposed_provenance_mut`. That breaks
ADR 0009. Path 2 is still unresolved.

**Size check.** The link fits every class. All 52 classes are multiples of
16 bytes and at least 16 (`CLASS_TO_SIZE` in `size_class/tables.rs`;
`MIN_BLOCK_SIZE`, `constants.rs:34`). `size_to_class(4)` is class 0 (16 bytes),
so the link write `[b, b+8)` stays inside the freed block.

The conflict is between the two aliasing models, not an overlap between
blocks. Evidence with PR #210's free-path change applied, on
`basic::test_basic_allocation` and on the CI facade filter:

- **Link written through the caller's pointer** (as committed):
  - SB: "trying to retag from <291278> for SharedReadWrite permission at
    alloc90842[0x17e684] … <291278> was created by a Unique retag at offsets
    [0x17e680..0x17e684]" (`basic.rs:8`, `drop(x)`).
  - TB: passes.
  - The 8-byte write extends past the 4-byte `Box<i32>` tag, inside the
    16-byte block.
- **Link written through mapping provenance** (one-line variant,
  `locate_block(ptr)` at `classified.rs:85`):
  - SB: facade 39/39.
  - TB: facade 0/39. The diagnostic is "write access through <281572> at
    alloc2412[0x1e31c0] is forbidden … the accessed tag <281572> is foreign
    to the protected tag <267055> … protected tags must never be Disabled".
  - <267055> was created at std `thread/lifecycle.rs:134`,
    `ThreadInit::init(self: Box<Self>)`.
  - The full backtrace shows the freed block is that same box:
    `Box<ThreadInit>::drop` → `__rust_dealloc` → `thread_free_cold` →
    `AtomicFreeList::push_dynamic` → `Block::set_next_raw`.
  - std frees its own argument box while that box's protector is active. Any
    write the allocator then makes through a tag that is not a child of the
    box's tag violates TB.

No single tag satisfies both models for a request narrower than the link
(1–7 bytes, all in class 0). The underlying semantics are an open opsem
question: rust-lang/unsafe-code-guidelines#442, "What about: user-defined
global allocators", covers provenance across the allocator boundary and
`dealloc` resetting memory.

Tracked by backlog item MN-LOCAL-MIRI-UB (`backlog.md#MN-LOCAL-MIRI-UB`).

## Options

Kernels were measured as release x86-64 instruction counts of isolated,
`#[inline(never)]` functions. Wall-clock is invalid on this host (88–100%
load), and iai-callgrind needs valgrind, which this Windows host lacks. The
kernel source is in this ADR's PR body (#213).

| Kernel | Instructions | Per 64 KiB page | Notes |
|---|---|---|---|
| In-band push / pop (today) | 4 / 6 | 0 B | writes into the freed block |
| In-band remote push (today) | 5 + `lock cmpxchg` loop | 0 B | ABA tag needed |
| (a1) `u16` index array push / pop | 10 / 13 | 2 B per slot: 8 KiB at class 0 (12.5%) | keeps LIFO order |
| (a2) bitmap + summary push / pop | 19 / 26 | 520 B local + 520 B remote (1.6%) | exact double-free check |
| (a2) remote push | 16, two `lock` RMWs, no loop | — | wait-free, no ABA |
| Segment by mask (today) | 3 | 0 B | needs the caller's provenance |
| Segment by two-level registry | 13, two dependent loads | 64 KiB static + 64 KiB per 16 GiB span | ADR 0009-conforming |

- **(a) Out-of-band free tracking.** The free list never writes into a freed
  block, so the link involves neither the caller's tag nor std's protector.
  The link is then sound under SB, TB and strict provenance by construction.
  Free-time poisoning is the one remaining in-block write (below).
  - **(a1) Index array.** Keeps LIFO order, the randomized and secondary
    lists, and the list algorithms. Costs 2/size of page capacity if the
    array is carved from the page (11.1% at 16 B, 1.6% at 128 B), or a
    fixed 8 KiB per page in the header.
  - **(a2) Bitmap.** Costs 1.6% of the segment for every class. Allocation
    order becomes first-free-by-address within a page. Free-list encryption
    has no in-band link left to protect. Remote frees become a wait-free
    `fetch_or`.
  - **(a0)** Out-of-band tracking for class-0 pages only, since requests of
    8 bytes or more always cover the link. This is the least memory, but it
    means two list representations and a class branch on every free.
- **(b) Treat TB as the allocator crates' aliasing oracle** and keep SB for
  the rest of the stack. This is recorded as an architectural decision citing
  UCG#442. Last resort: SB coverage of the allocator's free path is lost.
- **Free-time poisoning.** `SecurePolicy` and `HardenedPolicy` set
  `ENABLE_POISONING`, and they write `block_size` bytes, or the huge
  mapping suffix, through the caller's pointer (`classified.rs:81-82`,
  `free_helpers.rs:165-170`). That is the same SB conflict for narrowed
  tags, wherever the link lives. No measured run exercises it: the facade
  uses `StandardPolicy`.
  - Recommended: poison `layout.size()` bytes when the layout is known (the
    Rust `dealloc` path). Slack past the request never held user data, and
    the caller's tag covers the request.
  - C `free` keeps `block_size`, because no Rust retag reaches it.
- **Segment location.** Either:
  - **conform to ADR 0009** with a registry that maps each 2 MiB chunk to its
    mapping-derived segment pointer. Registered at map and unmap time, and for
    every chunk of a huge mapping. Costs +10 instructions and two dependent
    loads per free. Strict-provenance Miri stays clean.
  - or **revise ADR 0009** to allow exposed provenance in the
    `locate_segment`, `locate_block` and `locate_huge_back_pointer` entry
    points. This loses strict-provenance coverage: 5 of 36
    `mnemosyne-memory-core` tests become "unsupported" under
    `-Zmiri-strict-provenance`. The only replacement oracle is SB/TB Miri,
    whose wildcard provenance matches an access against any exposed tag, so
    it checks pointer identity less precisely. CI does not run strict
    provenance.

## Decision (recommended)

Adopt **(a2) bitmap** for all classes, together with the **registry**,
conforming to ADR 0009. This is the only combination that is clean under SB,
TB and strict provenance:

- no exposed provenance and no Miri flag or filter;
- no in-block link, and poisoning confined to the caller's bytes;
- 1.6% header memory instead of 12.5%;
- exact double-free detection and wait-free remote frees.

The cost is about +25 instructions on the free path. The precondition to
Accept is a counter-based churn benchmark showing first-free-by-address order
does not regress reuse locality against today's LIFO order. If it does, fall
back to (a1) under the same registry.

## Consequences

PR #210's exposed-provenance entry points are replaced and ADR 0009 stands.
`Block::next_encoded`, free-list encryption (ADR 0001, revised in the same
change) and the tagged remote head retire for small pages. Segment map and
unmap gain a registry publish and retire. MN-LOCAL-MIRI-UB stays open until
the implementation lands.

## Overturning evidence

A churn regression attributable to allocation order; a registry lookup that
dominates free on deterministic counters; or UCG#442 settling that a global
allocator's `dealloc` begins with fresh provenance, which would make the
in-band design sound.
