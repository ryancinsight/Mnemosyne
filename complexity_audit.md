# Complexity Audit

Per-component, per-operation asymptotic complexity of the Mnemosyne allocator.
Refreshed for the current 11-crate workspace after the bitmap-free-list
experiment was rejected, and after the per-segment-occupancy-mask, per-CPU-cache,
free-list-encryption, doubly-linked page-list, and module-split changes.

`n_p(c)` = pages of size class `c` held by one thread (active + full lists);
`n_s` = segments owned by one thread; `P` = `PAGES_PER_SEGMENT` (32, a
compile-time constant); `C` = `NUM_SIZE_CLASSES` (44); `k` = items in a
cross-thread batch. Compile-time constants are O(1) by definition; they appear
only to make the constant factor explicit.

## Workspace map (components)

| Crate | Role |
| :--- | :--- |
| `mnemosyne-core` | Layout types (`Block`/`Page`/`Segment`), size classes, validation, `AtomicFreeList`, policies |
| `mnemosyne-backend` | OS page backend (mmap/VirtualAlloc), telemetry, CUDA |
| `mnemosyne-arena` | Segment allocation, global/orphan pools, NUMA, large/huge |
| `mnemosyne-local` | Thread-local cache; alloc/free/realloc hot paths; page/segment management; per-CPU L1; TLS |
| `mnemosyne` | `#[global_allocator]` shell + telemetry endpoints |
| `mnemosyne-c-shim` | C ABI (`malloc` family) |
| `mnemosyne-hardened` | `SecurePolicy`/`HardenedPolicy` ZSTs (poison, zero-init, encryption, randomization) |
| `mnemosyne-heap` | Branded heaps (`brand`/`branded_box`/`branded_vec`) — compile-time aliasing safety |
| `mnemosyne-decay` | Background decay-purge engine |
| `mnemosyne-prof` | Sampling allocation profiler |

## Per-allocation / per-free hot path

| Operation | Component | Complexity | Mechanism |
| :--- | :--- | :---: | :--- |
| `size_to_class` / `round_up_size` | `core::size_class` | O(1) | piecewise bit-shift schedule (bounded 4-way) |
| `class_to_size` / `class_to_max_blocks` | `core::size_class` | O(1) | `const` lookup tables |
| `alloc` (initialized free list, fast) | `local` | O(1) | pop head of active page free list |
| `alloc` (fresh page bump) | `core::types::Page` | O(1) | `initialized_blocks` index computes the next block address directly |
| `free` (local, non-full) | `local::free` | O(1) | push onto `page.free`; **O(1) `page_occupied_mask` update only on 0↔nonzero transition** |
| `free` (per-CPU L1 hit) | `local::per_cpu` | O(1) | single CAS push/pop on a per-CPU class stack |
| `free` (cross-thread) | `local::free` | O(1) | single CAS onto `page.thread_free` |
| `alloc` cross-thread reclaim | `local` | O(k) → O(1) amortized | drain `thread_free`, relink; each block reclaimed once |
| `usable_size` | `local::usable_size` | O(1) | segment-rounding + one page-metadata read |
| `realloc` in-class | `local::realloc` | O(1) | `usable_size` fit check returns same pointer |
| `realloc` cross-class | `local::realloc` | O(n) | alloc + `copy_nonoverlapping(min)` + free |
| `current_cpu_id` (per-CPU hot path) | `local::per_cpu` | O(1) | `GetCurrentProcessorNumber` (Windows); `getcpu` syscall (Linux — `rdtscp`/`TSC_AUX` optimization pending) |
| free-list `get_next`/`set_next` | `core::types` | O(1) | optional XOR-with-per-page-key (encryption) — branchless |
| `GlobalAlloc::{alloc,dealloc}` | `mnemosyne` | O(1) | dispatch to the above under a ZST policy |

Every steady-state allocate/free is O(1). Small classes use the same
free-list/bump-page mechanics as other size classes; the rejected bitmap
experiment is not present in the current allocator.

## Headline improvement since the prior audit: `page_occupied_mask`

`Segment` now carries a `page_occupied_mask: u32` — one bit per page (P = 32
fits a `u32`). It is maintained in O(1) by `increment/decrement_alloc_count_for_segment`,
which flip the page's bit **only** on the `alloc_count` 0↔nonzero transition.
Consequences:

- **"Does this segment have any live allocations?"** is now a single `mask != 0`
  test — O(1), replacing the prior O(P) scan over all 32 pages in
  `try_reclaim_segment` and defrag.
- **Iterating occupied pages** uses `mask.trailing_zeros()` to visit only set
  bits — O(popcount(mask)) instead of O(P).
- **"Can this allocator release a spare segment?"** is now a single
  `owned_segment_count >= 4` check maintained by the intrusive owned-list
  insert/remove paths, replacing the prior bounded scan on each reclaim
  candidate.

## Management / cold path

| Operation | Component | Complexity | Notes |
| :--- | :--- | :---: | :--- |
| `unlink_owned_segment` | `local::local_alloc::segment` | O(1) | intrusive **doubly-linked** owned-segments list |
| page list `list_push` / unlink | `local::local_alloc` | O(1) | intrusive **doubly-linked** page lists (`prev`/`next`) |
| `try_reclaim_segment` | `local::local_alloc::segment` | O(popcount(mask)) | O(1) owned-count threshold check, then occupied-page walk using `trailing_zeros`; P remains a compile-time upper bound |
| `alloc_cold` full-list reclaim sweep | `local::local_alloc::routing` | O(1) | bounded to 128 probes (documented latency cap) |
| `get_new_page` (empty reuse / fresh slice) | `local::local_alloc::routing` | O(1) | pop empty list / append fresh page |
| `get_new_page` orphan adoption | `local::local_alloc::routing` | O(P) | one pass over a segment's pages; P constant |
| `stats` | `local::local_alloc::stats` | O(n_listed_pages) | diagnostic snapshot walks active/full/empty page lists and uses `owned_segment_count`; not on any allocation path |
| `reclaim_owned_segments` (thread exit) | `local::local_alloc::segment` | O(n_s · P) | once per thread teardown |
| `allocate_large_or_huge` / `deallocate` | `arena` | O(1)* | *direct backend mmap/munmap; head-slack `decommit` |
| `GlobalSegmentPool::{push,pop}` | `arena::segment::pool` | O(1) | lock-free CAS stack |
| `purge_segment_pool` / `reset_segment_pool` | `arena` | O(retained) | bounded by `MAX_RETAINED_SEGMENTS` (constant) |
| `decay_step` (background thread) | `decay` | O(orphan_segments · P) | off the allocation hot path; runs on a cadence |
| profiler `dump_profile` | `prof` | O(samples) | off hot path; sharded sample maps |

## Remaining super-constant operations (runtime-variable input)

1. `reclaim_owned_segments` / `decay_step` — O(n_s · P) or O(orphan · P).
   Teardown / background only; not optimization targets (running counters
   would push maintenance onto the hot path, a previously rejected trade — see
   `gap_audit.md`). The occupancy mask already lets these skip empty pages
   where reclamation semantics permit it. `stats` no longer scans every page in
   every owned segment; it walks the active/full/empty page lists.

## Zero-cost / ZST / monomorphization posture

- **Policies** (`StandardPolicy`/`SecurePolicy`/`HardenedPolicy`) are ZSTs whose
  capabilities (`ENABLE_POISONING`, `ZERO_INITIALIZE`,
  `ENABLE_FREE_LIST_ENCRYPTION`, `RANDOMIZE_ALLOCATION`) are associated `const`
  bools. Every policy branch is monomorphized and dead-code-eliminated per
  instantiation — no runtime policy dispatch on hot paths.
- **Backend** (`HasSegmentPool`/`MemoryBackend`) and **TLS slot** selection are
  trait/ZST parameters resolved at monomorphization; `dyn` does not appear in
  the allocator hot paths.
- **`mnemosyne-heap` brands** are ZST lifetime-tagged types (GhostCell-style)
  giving compile-time aliasing guarantees at zero runtime cost.
- Free-list encryption is a branchless XOR with a per-page key, gated by the
  policy `const`; `StandardPolicy` compiles it out entirely (key path is dead).
- Size-class mapping, structural bounds, and layout invariants are `const` /
  `const _: () = assert!(...)`, evaluated before code generation.

## Verification status (cross-reference)

- Per-component O(1) hot-path claims hold structurally, and the value-semantic
  guarantees they assume are currently **green**: `cargo test --workspace
  -- --test-threads=1` passes (33 test groups, 0 failures), including the
  `mnemosyne-local` lib suite that exercises the free-list, encryption,
  per-CPU, and page-recycling interaction. The earlier free-poison-vs-free-list
  corruption regression (a `0xDE` byte reaching an inline free-list `next`) has
  been resolved in the current `main`.
- Pages are now doubly linked (`prev_page`/`next_page`), so `unlink_page_from_list`
  splices in O(1) without a predecessor search — confirming the page-list O(1)
  conversion flagged as "planned" in the prior audit is now complete.
