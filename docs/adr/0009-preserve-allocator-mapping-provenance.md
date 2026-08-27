# ADR 0009: Preserve allocator mapping provenance

Status: Accepted

## Context

Mnemosyne aligns segment headers inside larger backend mappings and recovers a
segment and page from allocator-owned pointers. The original implementation
reduced pointers to integer addresses, masked or aligned those integers, and
cast them back to pointers. Cached page-list nodes and the 64-bit packed atomic
free-list head repeated the same pattern through exposed provenance.

Those operations preserve numeric addresses but discard the allocation
provenance required to access the complete mapping. The defect remained hidden
because wildcard provenance let Miri continue under its permissive model. With
strict provenance enabled, the Leto single-write storage constructor reached
three such recoveries before its first value-semantic assertion. An accessor-
only rewrite had previously failed because blocks and cached pages were still
produced from narrower page-scoped pointers; the accessors could not create the
missing proof after the fact.

The public receiver methods `Page::parent_segment`, `Page::page_start`,
`Page::initialize_free_list`, and `Page::pop_block` also formed or retained a
page-scoped reference while accessing parent-segment state. That reference
asserted exclusivity over metadata that remote frees may access atomically and
could not preserve the parent mapping's provenance.

Tracked by backlog item MN-436.

## Decision

Preserve the backend mapping pointer through the complete allocator pipeline:

- derive aligned segment, guard, page, and block pointers with `addr` plus
  `map_addr` or in-allocation pointer arithmetic;
- project page metadata with raw place expressions from the live segment
  pointer, and carry those raw pointers through page lists;
- store pointer-bearing atomic heads in `AtomicPtr`, including tagged 64-bit
  heads, so address tags change through `map_addr` without discarding the head
  node's provenance;
- encode free-list links by changing their address component with `map_addr`,
  retaining the linked block's provenance through encode/decode;
- replace page receiver APIs that cannot prove segment-spanning provenance with
  raw, segment-addressed associated functions.

The replacement API is intentionally breaking. Callers migrate as follows:

- `page.parent_segment()` becomes
  `Page::parent_segment_of(page_pointer)`;
- `page.page_start()` becomes
  `Page::page_start_in_segment(segment, page_index)`;
- `page.initialize_free_list(...)` becomes
  `Page::initialize_free_list_in_segment(segment, page_index, ...)`;
- `page.pop_block::<P>()` becomes `Page::pop_block::<P>(page_pointer)`.

All replacement operations are unsafe because the caller must supply a live
mapping-derived pointer and the matching in-range page index. No compatibility
wrapper remains: a receiver wrapper would recreate the invalid provenance or
aliasing contract this decision removes.

## Alternatives

**Retain exposed-provenance reconstruction.** Rejected. It keeps the allocator
dependent on the permissive model, prevents strict Miri from checking pointer
identity, and cannot justify parent-header access from a page-scoped pointer.

**Keep receiver wrappers around the segment-addressed functions.** Rejected.
The wrapper would still have to recover a parent mapping from `&self` and would
make an exclusivity claim that remote-free metadata access violates. A wrapper
that only forwards is not a compatibility mechanism; it preserves the defect.

**Store a parent pointer in every `Page`.** Rejected. It consumes another
pointer-sized field in a cache-line-bounded hot metadata record and duplicates
information already available from the mapping-derived pointer and stored page
index.

**Remove pointer tagging and encryption.** Rejected. ABA tagging and hardened
free-list links are current allocator contracts. `AtomicPtr` plus `map_addr`
preserves those contracts without provenance loss or additional storage.

## Consequences

Low-level external callers must migrate to the segment-addressed API. The type
signatures now make the required provenance source visible: callers cannot
obtain a parent segment or writable page storage from a page reference alone.

`AtomicFreeList` and segment-pool tagged heads retain their one-word layout and
their existing ordering and packed-count/tag semantics. Per-CPU cache entries
store `AtomicPtr<u8>` rather than pointer integers; the storage size is
unchanged.

Native concurrency stress and TSan remain the evidence for the deliberately
large segment-pool stress binaries. Interpreting those 80,000-plus atomic
operations under Miri exceeds the committed runtime bound and does not replace
the race detector or loom model. Strict Miri instead covers the core pointer
operations, arena mapping tests, and the exact Leto consumer path.

## Verification

The exact Leto `MnemosyneStorage` constructor parity/drop test passes under
`MIRIFLAGS=-Zmiri-strict-provenance` with no pointer reconstruction warning.
The `mnemosyne-memory-core` library suite passes 18/18 under the same model,
including free-list push/pop and randomized page initialization. Four focused
arena tagged-stack tests cover LIFO/count, chain splice, detach/count, and
active-observer synchronization under strict Miri. Native nextest passes
171/171 across core, arena, and local; hosted TSan run 32198740189 and loom
cover the native-sized concurrency stress boundary. Affected Clippy, doctests,
and warning-denied rustdoc pass.

`cargo-semver-checks` reports the expected major change: the three removed
receiver methods and `Page::pop_block`'s receiver loss. Arena and local pass
all 196 applicable compatibility checks.
