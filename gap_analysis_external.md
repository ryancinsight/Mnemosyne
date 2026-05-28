# External Gap Analysis: Mnemosyne vs. Production Allocators and Recent Research

This document complements `gap_audit.md` (which tracks *internal* defects against
the project's own invariants) by recording *external* gaps: features and
techniques present in production-grade allocators and recent allocator
research that Mnemosyne does not yet implement or has implemented partially.

Each row in the gap tables names (1) the feature, (2) the closest existing
state in Mnemosyne, (3) the source allocator or paper, (4) the architectural
implication, and (5) a recommended priority tag (`[arch]`, `[major]`,
`[minor]`, `[patch]`) consistent with `CLAUDE.md` versioning policy.

## 1. Reference set

- **mimalloc** (Daan Leijen, Benjamin Zorn, Leonardo de Moura — Microsoft
  Research, 2019; v2.x and v3.x active development). Design paper:
  *"Mimalloc: Free List Sharding in Action"*, MSR-TR-2019-18.
- **snmalloc** (Paul Lietar, Theodore Butler, Sylvan Clebsch, Sophia Drossopoulou,
  Juliana Franco, Matthew J. Parkinson, Alex Shamis, Christoph M. Wintersteiger,
  David Chisnall — Microsoft Research, 2019; v0.6+ rearchitecture).
  Design paper: *"snmalloc: A Message Passing Allocator"*, ISMM 2019.
- **jemalloc** (Jason Evans — originally FreeBSD libc, now Facebook). Design
  notes: *"A Scalable Concurrent malloc(3) Implementation for FreeBSD"*,
  BSDCan 2006; *"Scalable memory allocation using jemalloc"*, Facebook
  Engineering 2011.
- **tcmalloc** (Google; two active forks: gperftools tcmalloc and Google's
  current per-CPU tcmalloc). Design notes:
  *"TCMalloc: Thread-Caching Malloc"*.
- **Mesh** (Bobby Powers, David Tench, Emery D. Berger, Andrew McGregor —
  PLDI 2019). *"Mesh: Compacting Memory Management for C/C++ Applications"*.
- **Scudo** (LLVM project / Android Bionic hardened allocator). Design notes:
  *"Scudo Hardened Allocator"*, LLVM docs.
- **PartitionAlloc** (Chromium). Design notes:
  *"PartitionAlloc Design"*, Chromium source tree.

## 2. Gap matrix — small-allocation fast path

| Feature | Mnemosyne current state | Reference | Implication | Priority |
| --- | --- | --- | --- | --- |
| Per-page sharded free lists (`free`, `local_free`, `thread_free`) | Implemented; matches mimalloc semantics exactly | mimalloc | Parity achieved. | done |
| Cross-thread free queue per page | Implemented (page-local `AtomicFreeList`) | snmalloc | Parity achieved. | done |
| Single-cache-line page metadata (≤ 64 B) | 64 B exactly, pinned by `page_struct_size_stays_within_one_cache_line` | mimalloc page (64 B), snmalloc slab (64 B+) | Parity. | done |
| Bitmap free lists for very small classes | Not implemented (linked-list free lists for every class) | snmalloc 0.6 backend chunkmap | Bitmaps reduce per-block metadata for 16-byte class; cache-conflict risk. Investigate after profiling shows the 16/32 B classes are bandwidth-bound. | `[arch]` |
| Page-local free list compaction on free | Linked list LIFO | mimalloc shifts free→local_free→thread_free in batched flushes | Mnemosyne already does the three-list dance; check whether batched flush ordering matches mimalloc's restoration logic. | `[minor]` |
| Encrypted free list pointers (`next` XOR per-page secret) | Not implemented | mimalloc-secure, Scudo, hardened_malloc | Detects type confusion / linear UAF. Belongs in `SecurePolicy`. | `[minor]` |
| Per-page guard pages | Not implemented | hardened_malloc, Scudo, PartitionAlloc | Catches linear OOB writes. Requires `mprotect` plumbing in the backend. | `[major]` |
| Allocation-randomized first-fit | Not implemented (always LIFO from `page.free`) | hardened_malloc, Scudo | Increases UAF reuse latency; trades cache locality. Optional `SecurePolicy` knob. | `[minor]` |

## 3. Gap matrix — segment / arena layer

| Feature | Mnemosyne current state | Reference | Implication | Priority |
| --- | --- | --- | --- | --- |
| Aligned 2 MiB segments (matches x86 large-page boundary) | Implemented (`SEGMENT_SIZE = 2 MiB`, `SEGMENT_ALIGN = 2 MiB`) | mimalloc 4 MiB / snmalloc 16 MiB | Parity; smaller segments trade fragmentation for adoption granularity. | done |
| Bounded retained-segment pool (`MAX_RETAINED_SEGMENTS = PAGES_PER_SEGMENT = 32`) | Implemented | mimalloc page reset bound, snmalloc chunk reuse | Parity. | done |
| Explicit `purge()` (drains pool, releases to OS) | Implemented | mimalloc `mi_collect`, jemalloc `arena.purge` | Parity for explicit purge. | done |
| **Decay-based purging** (background gradual `madvise(MADV_DONTNEED)`) | Not implemented | jemalloc decay, mimalloc reset delay | Cuts RSS on idle workloads; needs a `Decay` policy + background thread or per-alloc bookkeeping. | `[major]` |
| **`MADV_DONTNEED` / `MADV_FREE` on idle pages** (not just whole segments) | Not implemented (only whole-segment `B::deallocate`) | jemalloc, tcmalloc, mimalloc page reset | Releases physical memory without giving up the virtual address. Belongs in `mnemosyne-backend` as a separate `page_reset` op. | `[major]` |
| Huge-page request flags (`MADV_HUGEPAGE`, `MAP_HUGETLB`, `MEM_LARGE_PAGES`) | `MADV_HUGEPAGE` hint implemented for Linux segment-sized mappings; `MAP_HUGETLB` / `MEM_LARGE_PAGES` not requested (require root / SeLockMemoryPrivilege) | jemalloc THP, mimalloc large page, tcmalloc HugePageHeap | `MADV_HUGEPAGE` parity; explicit `MAP_HUGETLB` remains a follow-on. | partial |
| Orphaned segment adoption | Implemented (`GLOBAL_ORPHAN_POOL`) | snmalloc message-passed alloc / mimalloc abandoned page list | Parity for thread-death; snmalloc adopts at sub-segment granularity. | done |
| Per-segment `raw_alloc_ptr` retained for unaligned OS frees | Implemented | mimalloc preserves original mmap pointer | Parity. | done |
| **Backend-side guard page in segment metadata prefix** | Not implemented | Scudo, hardened_malloc | Catches linear OOB writes from one page into the segment header. Cheap (one `mprotect` per segment). | `[major]` |

## 4. Gap matrix — large / huge allocations

| Feature | Mnemosyne current state | Reference | Implication | Priority |
| --- | --- | --- | --- | --- |
| Direct OS mapping for sizes above the small cap | Implemented (`allocate_large_or_huge`) | mimalloc, jemalloc, tcmalloc | Parity. | done |
| Tight worst-case mapping (`size + alignment + SEGMENT_ALIGN + PAGE_SIZE`) | Implemented and pinned by `huge_allocation_consumes_tight_mapping_size` | jemalloc base allocator, mimalloc huge | Parity with the tightest known derivation. | done |
| Power-of-two alignment validation before backend call | Implemented (`is_valid_alloc_request`) | All references | Parity. | done |
| Free classification by segment rounding (no side registry) | Implemented; alignments above `SEGMENT_ALIGN` rejected to preserve invariant | snmalloc chunkmap is a side registry; mimalloc uses segment rounding | Mnemosyne matches mimalloc's zero-copy classifier; snmalloc trades a small chunkmap for higher-alignment support. | done |
| **Huge-page-aware large allocation** (use `madvise(MADV_HUGEPAGE)` on the 2 MiB region) | Not implemented | jemalloc, mimalloc | TLB win on >2 MiB allocations. | `[minor]` |
| **Bounded retention of huge mappings** (recycle large pools instead of round-trip mapping) | Not implemented (every large alloc goes back to backend) | jemalloc retained extent cache | Saves mmap/munmap syscalls under churn. | `[minor]` |

## 5. Gap matrix — thread-local cache architecture

| Feature | Mnemosyne current state | Reference | Implication | Priority |
| --- | --- | --- | --- | --- |
| Thread-local cache via `thread_local!` macro | Implemented per backend via `LocalAllocatorSelector` | mimalloc, jemalloc TCs | Parity. | done |
| Re-entrancy guard (`IS_ALLOCATING` flag) | Implemented per backend | mimalloc reentrancy bit | Parity. | done |
| **Multiple heaps per thread (`mi_heap_t`)** | Not implemented (single TLS per backend) | mimalloc heap API, jemalloc arenas | Supports compartmentalization (per-request arenas, freed-as-a-group). Would require lifting `ThreadAllocator` out of TLS into an owned heap object. | `[arch]` |
| **Per-CPU caching** (rather than per-thread) | Not implemented | tcmalloc per-CPU, snmalloc 0.6+ per-CPU | Reduces TLS overhead in heavily threaded workloads; needs `restartable sequences` on Linux. | `[arch]` |
| **NUMA-aware arena selection** | Not implemented | jemalloc `arena.numa_node`, tcmalloc per-NUMA arenas | Cross-socket bandwidth wins on >16 core boxes. | `[major]` |
| Active/full page lists per size class | Implemented (`active_pages`, `full_pages`) | mimalloc full list | Parity. | done |
| Linear scan of `full_pages` in `alloc_cold` capped at 8 | Implemented; documented bound | mimalloc full-pages traversal | Parity. The 8-cap is a design tuning knob. | done |
| **Bucketed / ring-buffer recycle queue** instead of linear `try_recycle_page` | Linear scan over owned segments | mimalloc's `mi_segments_collect` is also linear but bounded by segment count; snmalloc uses per-class ring buffers | Reduces worst-case recycle latency under many owned segments. | `[minor]` |

## 6. Gap matrix — security & hardening

| Feature | Mnemosyne current state | Reference | Implication | Priority |
| --- | --- | --- | --- | --- |
| Compile-time `SecurePolicy` ZST | Implemented | mimalloc-secure build flag | Parity for opt-in. | done |
| Zero-on-allocation (`SecurePolicy::ZERO_INITIALIZE`) | Implemented | mimalloc-secure, Scudo, calloc-style | Parity. | done |
| Free-time poisoning (`SecurePolicy::POISON_FREE_BYTE = 0xDE`) | Implemented | mimalloc-secure, Scudo | Parity. | done |
| **Free-list pointer encryption (`next` XOR per-page secret)** | Not implemented | mimalloc-secure free-list encoding, Scudo cookie | Detects double-free and naive UAF link rewrites. | `[minor]` |
| **Chunk header checksum** | Not implemented | Scudo `Chunk::Header::Checksum` | Detects metadata corruption. Overhead vs. small classes is high. | `[minor]` |
| **Allocator stack canary / per-segment guard pages** | Not implemented | hardened_malloc, PartitionAlloc | OOB write detection. | `[major]` |
| **Randomized allocation ordering** | Not implemented | Scudo, hardened_malloc | UAF reuse latency. | `[minor]` |
| **Double-free detection (debug or always)** | Indirect: `debug_assert!(page.alloc_count >= count)` in `reclaim_thread_free` | mimalloc-debug, Scudo, jemalloc opt.junk | Mnemosyne catches metadata-inconsistent double-frees in debug builds but lacks per-block tagging that would catch them in release. | `[minor]` |
| **Memory tagging (ARM MTE, sparc ADI)** | Not implemented | Android Scudo MTE, hardened_malloc MTE | Hardware UAF/OOB detection on capable CPUs. | `[major]` |

## 7. Gap matrix — observability / introspection

| Feature | Mnemosyne current state | Reference | Implication | Priority |
| --- | --- | --- | --- | --- |
| Per-thread allocator stats | Implemented (`ThreadAllocatorStats`) | mimalloc `mi_stats_*`, jemalloc opt.stats_print | Parity. | done |
| Backend telemetry (mapped bytes, peak, map/unmap calls, purges) | Implemented (`BackendMemoryStats`, `ArenaMemoryStats`) | mimalloc reset stats, jemalloc decay stats | Parity. | done |
| Per-size-class occupancy snapshot | Implemented (`SizeClassOccupancy`) | mimalloc `mi_heap_visit_blocks`, jemalloc `arena.<i>.stats.bins` | Parity for class-level aggregation. | done |
| **Per-allocation profiling / heap snapshot** | Not implemented | jemalloc `prof.*`, mimalloc `mi_register_output` + custom profilers, tcmalloc heap profiler | Required for `pprof`-style flamegraphs; integrates with `tokio-console`, `eBPF`. | `[major]` |
| **Allocation backtrace tracking (opt-in)** | Not implemented | jemalloc with `libunwind`, Bytehound | Production debugging for leaks. | `[minor]` |
| **Tracing / event hook callback** (alloc/free user callback) | Not implemented | mimalloc `mi_register_output`, tcmalloc `MallocHook` | Enables flamegraphs without binary patching. | `[minor]` |
| **Per-heap RSS estimation** (resident vs. mapped) | Implemented for whole-process `current_mapped_bytes` but not per-heap RSS estimate | mimalloc `mi_heap_collect`, jemalloc `arena.<i>.stats.mapped` | Without page-reset support, RSS = mapped. | `[minor]` |

## 8. Gap matrix — configurability / tuning

| Feature | Mnemosyne current state | Reference | Implication | Priority |
| --- | --- | --- | --- | --- |
| Compile-time `AllocPolicy` selection | Implemented (`StandardPolicy`, `SecurePolicy`) | mimalloc compile flags | Parity for compile-time. | done |
| **Runtime environment-variable knobs** (`MIMALLOC_*`, `MALLOC_CONF`) | Not implemented (all constants are `const`) | mimalloc `mi_option_set`, jemalloc `MALLOC_CONF`, tcmalloc `TCMALLOC_*` | Enables production tuning without rebuild. Requires `OnceCell` for the runtime values plus a parse layer. | `[major]` |
| Per-deployment knobs: segment size, retention bound, purge cadence | Hardcoded constants | mimalloc all tunable, jemalloc all tunable | Trade-off: const = zero cost, env = flexibility. Compromise: `extern const`-style override block. | `[minor]` |
| **Custom per-heap configuration at runtime** | Not implemented | mimalloc `mi_heap_new_in_arena`, jemalloc per-arena opts | Multi-tenant workloads. | `[arch]` |

## 9. Gap matrix — interoperability

| Feature | Mnemosyne current state | Reference | Implication | Priority |
| --- | --- | --- | --- | --- |
| `#[global_allocator]` for Rust | Implemented (`Mnemosyne`, `MnemosyneAllocator<P, B>`) | jemallocator, mimalloc-rs, snmalloc-rs | Parity. | done |
| **C ABI (`malloc`/`free`/`calloc`/`realloc`)** | Not implemented | jemalloc, mimalloc, snmalloc all ship C shims; mimalloc-rs / snmalloc-rs offer optional C-API | Enables `LD_PRELOAD` and use from C/C++ code linked into Rust binaries. | `[major]` |
| **`posix_memalign` / `aligned_alloc` / `memalign`** | Indirectly via `GlobalAlloc` Layout | mimalloc, jemalloc | Same scope as C ABI above. | `[major]` |
| **`malloc_usable_size`** | Not exposed | mimalloc `mi_usable_size`, jemalloc `malloc_usable_size` | Lets Rust `Vec` shrink without reallocating. | `[minor]` |
| Custom backend (`MemoryBackend` trait, `HasSegmentPool` trait) | Implemented; CPU + CUDA backends shipped | mimalloc has `mi_os_*` indirection; jemalloc has `extent_hooks` | Parity for opt-in backend; CUDA backend exceeds typical allocator scope. | done |
| **HSA / ROCm / Metal / vRAM backends** | Not implemented (CUDA only beyond OS) | None at parity level; jemalloc has external arena hooks | Differentiator if implemented. | `[minor]` |

## 10. Gap matrix — recent research

| Technique | Mnemosyne current state | Reference | Implication | Priority |
| --- | --- | --- | --- | --- |
| **Compacting / page meshing** (relocate matching pages to drop fragmentation) | Not implemented | Mesh (PLDI 2019) | Reduces RSS on long-running fragmenting workloads. Requires re-mapping with `mremap` and a coordinator. | `[arch]` |
| **Statically partitioned heaps** | Not implemented (single per-thread cache) | Theseus, IsoHeap | Compile-time isolation per type. | `[arch]` |
| **ML-driven allocation hints** | Not implemented | OptiMalloc, LLAMA | Research stage. | `[minor]` |
| **Reuse distance estimation for purge cadence** | Not implemented (manual purge only) | jemalloc decay-based purging | Improves RSS vs. perf tradeoff. | `[major]` |
| **Lock-free per-CPU cache (uses `rseq` on Linux)** | Not implemented | Google TCMalloc per-CPU mode | Removes thread cache TLS overhead. | `[arch]` |
| **Single-instruction free-list pop (cmov-free path)** | Implemented (allocation reads `page.free` and updates inline) | mimalloc fast path is ~10 instructions | Parity. | done |
| **Free-list deferred zeroing** | Not implemented (free zeroes via `write_bytes`) | mimalloc DEEP_RANDOMIZE deferred zero | Reduces free-time cost by deferring zero work to allocation. | `[minor]` |
| **Capability-based metadata (ARM Morello, CHERI)** | Not implemented | CHERI allocator research | Architectural research. | `[arch]` |

## 11. Cross-cutting observations

1. **Mnemosyne's structural design is at parity with mimalloc's small-allocation
   fast path** (three-list sharded free queue, page-local cross-thread queue,
   64-byte page metadata, segment-rounding free classification, segment-pool
   retention, orphaned-segment adoption). The gaps cluster in three areas:
   *security hardening* (free-list encryption, guard pages, randomized order,
   memory tagging), *runtime configurability* (env knobs, multi-heap API), and
   *RSS management* (decay-based purging, per-page reset, MADV_DONTNEED on
   idle pages).

2. **The `MemoryBackend` trait already provides the seam needed for guard
   pages and per-page reset** — a `page_reset(ptr, size)` and `make_guard(ptr,
   size)` method on the trait would let `mnemosyne-arena` invoke them
   without changing the policy ZSTs. This is the lowest-friction expansion
   path for both the security and RSS gaps.

3. **The `SecurePolicy` ZST is the natural site for free-list encryption
   and randomized first-fit**, because both are payload-byte costs that
   should be ZST-gated rather than runtime-flagged. Adding a third policy
   (`HardenedPolicy`) layered on top of `SecurePolicy` is a clean extension.

4. **The C ABI gap is the largest gap to external adoption**, but is a
   significant ongoing-maintenance commitment (libc-interpose ordering, weak
   symbol layout, `dlsym` interaction). It belongs to `[arch]` not because
   it changes Mnemosyne internals but because it adds a new public surface.

5. **NUMA-awareness and per-CPU caching are research-grade scope changes**
   that would require restructuring `LocalAllocatorSelector` and the
   segment pool. They should not be attempted until the basic per-thread
   path is benchmark-stable against mimalloc on the same workloads — the
   project's existing reverts on TLS-collapse experiments show that small
   changes here are easy to regress.

6. **Recent research (Mesh, ML-driven, CHERI)** sits outside the production
   allocator scope and should be tracked as long-term `[arch]` candidates
   only.

## 12. Recommended priority queue (next four sprints)

| Sprint slice | Item | Priority | Test guard |
| --- | --- | --- | --- |
| Sprint A | Add `MemoryBackend::page_reset(ptr, size) -> bool` method; thread it through `mnemosyne-arena` so purge can release individual idle pages without freeing the whole segment. | `[minor]` | `release_telemetry_records_page_reset_decrement` |
| ~~Sprint A~~ **delivered (`9e89cf2`)** | `madvise(MADV_HUGEPAGE)` hint in `UnixBackend::allocate` when `size >= SEGMENT_SIZE && size % SEGMENT_SIZE == 0`. Linux-gated; advisory. | `[patch]` | `segment_sized_allocation_survives_hugepage_hint`, `sub_segment_allocation_skips_hugepage_hint` |
| Sprint B | Implement `HardenedPolicy` ZST with XOR-encoded free-list `next` pointers (key per page from a TLS seed). Layer over `SecurePolicy`. | `[minor]` | `hardened_policy_detects_freelist_tamper`, `hardened_policy_round_trip_alloc_free` |
| Sprint B | Add `Backend::make_guard(ptr, size) -> bool` and place a guard page at the end of every segment's reserved Page 0 prefix. | `[major]` | `guard_page_traps_segment_header_overrun` |
| Sprint C | Add a `MnemosyneOptions` runtime configuration struct exposing `max_retained_segments`, `purge_cadence_ms`, `enable_hugepage_hint`. Parse `MNEMOSYNE_*` env vars at first allocation. | `[major]` | `runtime_options_override_default_retention` |
| Sprint C | Add C ABI shim crate `mnemosyne-c-shim` exposing `malloc`/`free`/`calloc`/`realloc`/`posix_memalign`/`aligned_alloc`/`malloc_usable_size`. | `[major]` | `c_shim_round_trip_matches_global_alloc` |
| Sprint D | Background decay-based purge: a single low-priority thread drains the segment pool on a configurable cadence with the runtime knob from Sprint C. | `[major]` | `decay_purger_reaches_steady_state` |
| Sprint D | Add `mi_heap_t`-style multi-heap API: `MnemosyneHeap<P, B>` owning its own `ThreadAllocator`, with `MnemosyneHeap::alloc(&self, layout)`. Default `Mnemosyne` global allocator delegates to a TLS heap. | `[arch]` | `multi_heap_isolates_allocation_streams`, `multi_heap_release_does_not_touch_other_heaps` |

## 13. Items deliberately out of scope

- **Compacting / meshing.** Requires relocating user payloads, which violates
  the Mnemosyne contract that `*const T` pointers handed out are stable.
- **ML-driven allocation hints.** Research stage; no production allocator
  has integrated this.
- **CHERI / capability-based metadata.** Hardware not in mass deployment.
- **Per-CPU caching with `rseq`.** Linux-only kernel feature; would require
  a backend split that is not currently warranted.

## 14. Maintenance

Treat this document as a snapshot. Refresh quarterly by:

1. Re-reading the latest mimalloc, snmalloc, jemalloc, and tcmalloc release
   notes for new features.
2. Scanning ISMM, PLDI, OSDI, and ASPLOS proceedings for allocator papers
   since the last refresh.
3. Re-evaluating each `Priority` column against the current Mnemosyne
   feature set and benchmark position.

Cross-link any closure of a gap row to its `gap_audit.md` entry (which
records the *defect* closure) and its `CHANGELOG.md` row (which records the
*release* note).
