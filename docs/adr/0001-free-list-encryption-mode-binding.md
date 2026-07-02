# ADR 0001 — Binding free-list encryption mode to avoid mixed-policy corruption

- Status: Proposed (awaiting sign-off; blocks backlog AR-1 implementation)
- Change class: [arch]
- Date: 2026-07-01
- Scope: `mnemosyne-core` (`policy`, `types::page`, `types::segment`, `sync`),
  `mnemosyne-local` (`alloc`, `free`, `realloc`, `local_alloc::*`), the
  `thread_alloc`/`thread_free`/`thread_realloc` free-function surface.

## Context

`AllocPolicy` (a sealed ZST trait) carries `const ENABLE_FREE_LIST_ENCRYPTION`.
When true, a page's intrusive free-list `next` links are XOR-encoded with a
per-page cookie (`Segment::keys[page_index]`, derived from a per-thread seed);
`HardenedPolicy` sets it, `StandardPolicy`/`SecurePolicy` do not.

The per-thread `ThreadAllocator<B>` is keyed **only by backend `B`** (see
`impl_local_allocator_selector!` in `mnemosyne-local/src/lib.rs`): the TLS slot
is selected by `B`, and the policy `P` is an **independent** generic supplied
per call to `thread_alloc::<P, B>` / `thread_free::<P, B>`. One allocator
instance is therefore shared by every policy that targets a given backend, and
its `active_pages[class]` list hands the same page to allocations made under
different `P`.

Every hot-path encode/decode selects the mode from the **caller's** policy as a
compile-time constant:

- `Page::pop_block::<P>` (core `types/page.rs:310`), `Block::set_next::<P>`,
  `Page::initialize_free_list::<P>`, `AtomicFreeList::push::<P>`
  (core `sync.rs:80,165`), and the local free paths
  (`free.rs:160,328`) all branch on `P::ENABLE_FREE_LIST_ENCRYPTION`.

Because `P::ENABLE_FREE_LIST_ENCRYPTION` is `const`, dead-code elimination
strips the branch: `StandardPolicy` compiles to "cookie = 0, no segment
touch". This is the intended zero-cost behavior and it is load-bearing — the
small-alloc fast path (`pop_block`) does **not** otherwise read the segment
header (a different cache line from the page metadata), so the default policy
never pays a segment-header access.

The segment additionally carries a **dynamic** `free_list_encrypted: bool`.
The cold sweep/reclaim paths (`local_alloc::segment::reclaim`) correctly decode
using this dynamic flag; the hot paths do not.

### The defect (AR-1)

Two policies with different `ENABLE_FREE_LIST_ENCRYPTION` on one backend share
class pages, so a single page's chain can carry links written under different
modes:

1. `thread_alloc::<HardenedPolicy, B>` initializes page *p* (class *c*):
   `free_list_encrypted = true`, links encoded with keys.
2. `thread_alloc::<StandardPolicy, B>` pops from the same `active_pages[c]`;
   a subsequent `thread_free::<StandardPolicy, B>` of a block in *p* writes an
   **unencoded** link (`set_next::<StandardPolicy>`, cookie 0) into *p*'s
   encoded chain.
3. `pop_block::<HardenedPolicy>` decodes that link with the keys → a wild
   pointer → the in-page bounds check aborts (or, if the key delta's high bits
   vanish, silent free-list corruption).

This is reachable from the **public** `thread_alloc`/`thread_free`
free-function API, within a single thread, without `unsafe` at the call site.
The `policy_integration_tests` exercise Standard + Secure + Hardened over one
`MemoryBackendWrapper` but only bump-allocate (they never pop a mixed chain),
so the defect is currently latent. The 2026-07-01 cycle closed the
**cross-thread orphan-adoption** instance (`acquire_policy_compatible_segment`
defers policy-mismatched orphans and never re-keys a live segment); this ADR
covers the remaining **same-allocator same-page** instance.

## Options

### A. Honor the segment's dynamic flag on every encode/decode path

Replace `P::ENABLE_FREE_LIST_ENCRYPTION` with `(*segment).free_list_encrypted`
in `pop_block`, `set_next`, `initialize_free_list`, and the local free paths.

- Sound: one page has one authoritative mode; any policy reads it correctly.
- **Rejected as the primary path**: adds a segment-header load + a
  runtime branch to the `StandardPolicy` alloc fast path (`pop_block`), which
  today touches only page metadata. That is a measured-cache-line regression
  for the default, most-common policy — it fails the zero-cost bar. (The free
  path already loads the segment header for the owner check, so option A is
  nearly free *there*; the asymmetry is the alloc path.)

### B. Constrain `P` and `B` to agree, checked at compile time

Add `const ENABLE_FREE_LIST_ENCRYPTION` to the backend/selector trait and
`const { assert!(P::ENABLE_FREE_LIST_ENCRYPTION == B::ENABLE_FREE_LIST_ENCRYPTION) }`
in each generic entry point (a stable post-monomorphization const assertion).

- Zero runtime cost; keeps the const-folded hot path; a mismatch is a compile
  error.
- **Rejected**: encryption is a *safety-policy* property, not a *memory-source*
  property. Forcing it onto backend identity means `StandardPolicy` and
  `HardenedPolicy` could not share `MemoryBackendWrapper` — every deployment
  wanting hardening would need a parallel backend type and a second set of
  global pools. Semantically wrong and a large API break.

### C. Key the TLS allocator (and its pages) by encryption class — RECOMMENDED

Make the `ThreadAllocator` monomorphic in the encryption bit so a single
allocator instance owns only same-mode pages, and mixing is unrepresentable:

- Select the TLS allocator by `(B, {P::ENABLE_FREE_LIST_ENCRYPTION})` — one
  extra slot per backend, instantiated only when a second mode is actually
  used. `thread_alloc::<P, B>` picks the slot from `P`'s const bit, so alloc
  stays fully const-folded and zero-cost per instance.
- The **free** path recovers the owning allocator/segment and decodes using
  the segment's dynamic `free_list_encrypted` flag (already loaded on the free
  path for the owner check — near-free), so a `thread_free::<P', B>` with a
  mismatched `P'` still decodes correctly instead of corrupting the chain.
  This is the one place option A is cheap, and C adopts it there only.
- Cross-thread frees route through the owning page's `AtomicFreeList`, which
  must encode with the **segment's** mode (dynamic flag), not the freeing
  thread's `P` — same near-free treatment (the push already touches the
  segment for the cookie when encrypting).

Result: alloc fast path unchanged (const, zero segment touch); free/cross-free
use the dynamic flag they already have hot; pages never mix modes because each
allocator instance is single-mode and owns its pages. Sound and zero-cost for
the default policy.

Cost: moderate — a second TLS slot dimension, and the free/reclaim/cross-free
decode switched to the dynamic flag (partly already done in the sweep paths).
The per-backend static pools are shared across encryption classes (segments
are still returned to the backend's pool), so a segment moving between classes
must be re-initialized (`free_list_encrypted` reset) on reuse — which the free
pool's `Segment::initialize` already does, and which
`acquire_policy_compatible_segment` already gates for orphans.

## Decision

Adopt **Option C**. It is the only option that is both sound for the mixed-
policy case and zero-cost for the default (`StandardPolicy`) hot path, without
conflating safety policy with backend identity.

## Consequences / implementation plan (post-sign-off)

1. **Interim safeguard (independent, ship first, zero release cost):** add a
   `debug_assert_eq!((*segment).free_list_encrypted, P::ENABLE_FREE_LIST_ENCRYPTION)`
   at the free-path site where the segment is already loaded, and a matching
   debug assert on the alloc reclaim path, so any mixed-mode page aborts loudly
   in debug/CI builds while release stays untouched. Add a `#[should_panic]`
   (debug) or mode-mismatch test that pops a deliberately mixed page. This
   converts the silent UB into a caught invariant violation immediately.
2. Extend the selector trait/macro to a `(B, ENCRYPTION)` slot key; route
   `thread_alloc`/`thread_realloc` by `P`'s const bit.
3. Switch `thread_free_classified`'s owner path, `do_local_free_internal`,
   `AtomicFreeList::push`, and the cross-thread reclaim to decode/encode via
   the segment's dynamic `free_list_encrypted` flag (consolidate with the
   existing sweep-path dynamic decode — pairs with the AR-6 `cookie_for`
   accessor).
4. Verify with an interleaved Standard+Hardened same-page alloc/free/realloc
   test (value-semantic: the chain round-trips and no abort fires) and a
   criterion check that the `StandardPolicy` alloc fast path is unchanged
   (codegen/differential).

Until sign-off and steps 2–4 land, the contract is documented on the
`thread_*` functions: **all allocations and frees through a given backend must
use one `ENABLE_FREE_LIST_ENCRYPTION` setting**, and step 1's debug assert
enforces it in tests.
