# Complexity Audit

Per-component, per-operation asymptotic complexity of the Mnemosyne allocator.
`n_p(c)` = number of pages of size class `c` held by one thread (active + full
lists); `n_s` = number of segments owned by one thread; `P` =
`PAGES_PER_SEGMENT` (compile-time constant, 32); `C` = `NUM_SIZE_CLASSES`
(compile-time constant, 44). Compile-time constants are O(1) by definition; they
appear below only to make the constant factor explicit.

## Hot path (per allocation / per free)

| Operation | Component | Complexity | Mechanism |
| :--- | :--- | :---: | :--- |
| `size_to_class` | `mnemosyne-core::size_class` | O(1) | `const` lookup table indexed by size |
| `class_to_size` | `mnemosyne-core::size_class` | O(1) | `const` table index |
| `ThreadAllocator::alloc` (fast) | `mnemosyne-local` | O(1) | pop head of `active_pages[class].free` |
| `ThreadAllocator::alloc` (reclaim) | `mnemosyne-local` | O(k) in reclaimed blocks | drain `thread_free`, relink; amortized O(1) per freed block |
| `thread_free` local (non-full, current) | `mnemosyne-local` | O(1) | push onto `page.free`, decrement counter |
| `thread_free` cross-thread / re-entrant | `mnemosyne-local` | O(1) | single atomic push onto `page.thread_free` |
| `usable_size` | `mnemosyne-local` | O(1) | segment-rounding + one page-metadata read |
| `AtomicFreeList::{push,pop_all,is_empty}` | `mnemosyne-core::sync` | O(1) | single CAS / swap / load |
| `GlobalAlloc::{alloc,dealloc}` | `mnemosyne` | O(1) | dispatch to the above under a ZST policy |
| TLS slot access (`nightly_tls`) | `mnemosyne-local` | O(1) | single segment-register-relative load |
| TLS slot access (stable) | `mnemosyne-local` | O(1) | `LocalKey::with` (call + init-flag check) |

All steady-state allocate/free operations are O(1). The `alloc` reclaim branch
is O(k) in the number of blocks another thread freed since the last reclaim,
which amortizes to O(1) per freed block (each block is reclaimed exactly once).

## Cold / management path

| Operation | Component | Complexity | Notes |
| :--- | :--- | :---: | :--- |
| `thread_free` full→active transition | `mnemosyne-local` | **O(n_p(c))** | `unlink_full_page` walks `full_pages[class]` to find predecessor |
| `alloc_cold` full-list reclaim sweep | `mnemosyne-local` | O(1) | bounded to 8 probes (`checked >= 8`); intentional latency bound |
| `alloc_cold` new-page slice | `mnemosyne-local` | O(1) | append one fresh page from current segment |
| `get_new_page` empty-page reuse | `mnemosyne-local` | O(1) | pop head of `empty_pages` |
| `get_new_page` orphan adoption | `mnemosyne-local` | O(P) | one pass over a segment's pages; P is constant |
| `get_new_page` fresh segment | `mnemosyne-local` | O(1)* | *amortized; OS mmap cost excluded |
| `try_reclaim_segment` | `mnemosyne-local` | O(P + P·n_p(c) + n_s) | P-page reclaim scan + per-page `unlink_page` + `unlink_owned_segment` |
| `unlink_page_from_list` | `mnemosyne-local` | **O(n_p(c))** | singly-linked search for predecessor |
| `unlink_owned_segment` | `mnemosyne-local` | **O(n_s)** | singly-linked search for predecessor |
| `stats` | `mnemosyne-local` | O(n_s · P) | diagnostic snapshot; not on any allocation path |
| `reclaim_owned_segments` (thread exit) | `mnemosyne-local` | O(n_s · P) | runs once per thread teardown |
| `allocate_large_or_huge` / `deallocate_large_or_huge` | `mnemosyne-arena` | O(1)* | *direct backend mmap/munmap |
| `GlobalSegmentPool::{push,pop}` | `mnemosyne-arena` | O(1) | lock-protected stack push/pop |
| `purge_segment_pool` / `reset_segment_pool` | `mnemosyne-arena` | O(retained) | bounded by `MAX_RETAINED_SEGMENTS` (constant) |

## Super-constant operations and their input

Three operations exceed O(1) in a *runtime-variable* quantity (not a
compile-time constant):

1. `unlink_page_from_list` — **O(n_p(c))**. The hottest of the three: invoked on
   every full→active page transition in `thread_free` and on every page during
   `try_reclaim_segment`. `n_p(c)` is small in steady state (typically 1–3) but
   unbounded under a single thread holding many pages of one class with churn.
2. `unlink_owned_segment` — **O(n_s)**. Invoked once per segment reclamation in
   `try_reclaim_segment`.
3. `stats` / `reclaim_owned_segments` — **O(n_s · P)**. Diagnostic and
   thread-exit only; not optimization targets (running counters would push
   maintenance onto the hot path, a previously rejected trade — see
   `gap_audit.md`).

## Reduction plan: O(n) → O(1) for unlink operations

The canonical fix (mimalloc page queues) is **intrusive doubly-linked lists**:
store a `prev` pointer so a known node splices out in O(1) without searching for
its predecessor.

- **Owned-segments list** (`unlink_owned_segment`, target 2): `Segment` is
  multi-kilobyte metadata with no cache-line budget, so adding
  `prev_owned_segment` is free. Converting this list to doubly-linked makes
  `unlink_owned_segment` O(1) and removes the `n_s` term from
  `try_reclaim_segment`. **Implemented in this increment.**
- **Page lists** (`unlink_page_from_list`, target 1): `Page` is exactly 64 bytes
  (8 × 8-byte fields), fully consuming its single-cache-line budget enforced by
  `page_struct_size_stays_within_one_cache_line`. Adding `prev_page` requires
  freeing 8 bytes by deriving `page_index` from the page's own metadata address
  (`(page_addr − pages_base) / size_of::<Page>()`, O(1) since `size_of::<Page>()`
  is a power of two). This is a larger change touching ~10 intrusive-list splice
  sites; it will be landed as its own increment behind a single SSOT
  doubly-linked-list helper (push/unlink) so `prev`/`next` maintenance lives in
  one verified place, and gated on the no-regression discipline. A prior
  counter-compaction experiment that shrank `Page` fields regressed benchmarks
  (`gap_audit.md`), so the layout change is deliberately conservative: derive,
  do not narrow. **Foundation landed:** `Page::index_in_segment()` implements the
  O(1) address derivation and `page_index_field_matches_address_derivation`
  proves it equals the stored field for every page of a real segment, so the
  `prev_page` swap is de-risked; the full splice-site conversion is the remaining
  increment.

## C ABI shim (`mnemosyne-c-shim`)

Every exported `extern "C"` entry point is O(1) plus the O(1) thread-local
allocator path it forwards to. `n` below is the requested byte count.

| Operation | Complexity | Notes |
| :--- | :---: | :--- |
| `malloc` / `free` | O(1) | forward to `thread_alloc` / `thread_free` |
| `calloc` | O(n) | `checked_mul` (O(1)) + mandatory `write_bytes(0)` over the request; the zeroing is inherent to the C contract (Mnemosyne does not track zeroed pages) |
| `realloc` (in-class grow / shrink) | O(1) | `usable_size` check returns the same pointer when `new_size <= current_usable` |
| `realloc` (cross-class grow) | O(n) | one `malloc` + `copy_nonoverlapping(current_usable)` + `free`; copy length is exactly `current_usable` (the `min` branch was dead and was removed) |
| `aligned_alloc` / `posix_memalign` | O(1) | power-of-two/contract validation (O(1)) + `thread_alloc` |
| `malloc_usable_size` | O(1) | segment-rounding classification + one page-metadata read |

The shim holds no domain logic: it only converts arguments, applies the C/POSIX
contracts (overflow, alignment, `malloc(0)`, `realloc(p,0)`), and forwards under
the ZST `StandardPolicy` + `MemoryBackendWrapper`, so all dispatch is
monomorphized.

## Backend (`mnemosyne-backend`)

All `MemoryBackend` operations are O(1) at the Rust layer (one syscall each); the
kernel's own cost is excluded. Telemetry counters are single relaxed atomic adds.

| Operation | Complexity | Mechanism |
| :--- | :---: | :--- |
| `allocate` / `deallocate` | O(1) | `mmap`/`munmap` (Unix), `VirtualAlloc`/`VirtualFree` (Windows) |
| `page_reset` | O(1) | `madvise(MADV_DONTNEED/FREE)` / `VirtualAlloc(MEM_RESET)` (keeps commit) |
| `decommit` | O(1) | `madvise(MADV_DONTNEED)` / `VirtualFree(MEM_DECOMMIT)` (drops commit charge); used to return aligned-mapping head slack |
| `make_guard` | O(1) | `mprotect(PROT_NONE)` / `VirtualProtect(PAGE_NOACCESS)` |
| `AtomicFreeList::{push,pop_all,is_empty}` | O(1) | single CAS / swap / load on a tagged `AtomicUsize` (head + 16-bit count packed); `pop_all` returns the block count without walking |

## Zero-cost / monomorphization posture

- Policy selection (`StandardPolicy`/`SecurePolicy`), backend selection
  (`ComputeBackend`/`HasSegmentPool`), and TLS selection are ZST/trait
  parameters resolved at monomorphization; no runtime dispatch on hot paths.
- `dyn` dispatch appears only at the GUI/app-shell boundary (not present in the
  allocator core) per the documented zero-cost exception.
- Size-class mapping, structural bounds, and layout invariants are `const` /
  `const _: () = assert!(...)`, evaluated before code generation.
