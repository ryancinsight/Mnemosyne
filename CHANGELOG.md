# Changelog

## Unreleased

### Fixed

- `mnemosyne-local` now builds cleanly on the Atlas consumer test surface after
  the allocator reclaim-counter consolidation: the hot allocation reclaim path
  passes the thread-local `cross_thread_reclaimed` sink, free/reclaim call sites
  use the canonical branded list mover, test fixtures implement the current
  `BackendPools`-based `HasSegmentPool` contract, and `realloc` imports the
  core `locate_segment` SSOT used by the small-realloc path.
- Orphan-segment adoption no longer re-keys encrypted free lists: an adopted
  orphan's live chains are encoded with the keys already in its header (read
  concurrently by remote freeing threads), so `push_owned_segment` keys only
  never-encoded segments, and adoption gates on policy compatibility —
  a mismatched orphan returns to the pool for a matching-policy thread
  (`acquire_policy_compatible_segment`). Previously a hardened-policy thread
  adopting any encrypted orphan corrupted its free lists (abort on the bounds
  check) and raced concurrent key readers. Regression tests prove the encoded
  chain decodes end-to-end and the mismatch skip (verified failing under the
  old behavior).
- `TaggedSegmentStack::pop` retried with a `Relaxed` CAS-failure ordering while
  dereferencing the returned head's `next_free_segment` — an unsynchronized
  read against `push`'s Release publication (data race on weakly-ordered
  targets). The failure ordering is now `Acquire`.
- `mnemosyne-prof`: interned stack-slice hashes depended only on the last frame
  (`write_usize` replaced state instead of mixing), collapsing the intern map
  to one collision chain under the global mutex; frees while profiling was
  disabled never evicted resident samples (false leak reports plus a permanent
  cold-path tax on every free); concurrent hook/flag registrars could strand
  the aggregate active flag stale; and `on_alloc` passed the leak-detector
  flag inverted, letting a stale sampling budget hide allocations from leak
  tracking. All four fixed with value-semantic regression tests.
- `mnemosyne-decay`: a `configure()` racing the dying purger between its
  zero-cadence read and `SPAWNED` release could leave a non-zero cadence with
  no purger thread. The shutdown handshake now releases, re-checks, and
  re-claims via RMWs meeting in `SPAWNED`'s modification order.
- CUDA backend: `cuInit` probe state moved from cross-thread `static mut` to
  atomics; the registry unregister scan no longer early-exits on a stale count
  snapshot (a concurrent register could hide the target slot, permanently
  leaking the device allocation); init-race losers yield after a bounded spin;
  docs state the real null-on-unavailable contract (no host fallback exists).
- `mnemosyne_dump_leaks` saturates its count at `i32::MAX` instead of a
  wrapping cast that could collide with the `-1` error sentinel.

### Added

- `fuzz/c_shim_api`, a cargo-fuzz target for the `mnemosyne-c-shim` ABI over
  arbitrary `(op, size, nmemb, alignment)` inputs. The resource-bounded executor
  drives the real exported `malloc`/`calloc`/`realloc`/`aligned_alloc`/
  `posix_memalign`/`malloc_usable_size` functions and asserts null-or-valid
  allocation contracts, alignment, usable-size lower bounds, zeroed calloc
  prefixes, and initialized realloc preservation.
- Default `parallel` and `mnemosyne-memory` feature contracts across every
  Mnemosyne package. Leaf crates expose zero-dependency marker features; the
  top-level `mnemosyne/mnemosyne-memory` feature preserves the branded
  heap-backed default memory surface.
- `mnemosyne-core::kernel_budget` (atlas ADR 0002): `KernelResourceBudget`
  (registers/thread, shared-mem/block, threads/block; zero-thread launches
  rejected at construction) and `OccupancyLimits` with fully-`const`
  per-resource limiters and a `blocks_per_unit` binding-constraint minimum.
  This is the budget *vocabulary* GPU occupancy planning consumes — GPU
  compilers assign registers, so nothing here allocates; capacities arrive
  as plain quantities (themis `GpuTopology` accessor values, keeping
  mnemosyne-core `no_std`/dependency-free), and unreported capacities
  surface as `u32::MAX` "no information" rather than a fabricated bound.

### Breaking

- Sourced all brand machinery from the [`melinoe`](https://github.com/ryancinsight/melinoe) crate, making it the single source of truth for the ecosystem's brand identity and capability tokens. `mnemosyne-heap` no longer defines its own `Invariant<'brand>` marker or `AllocatorToken<'brand>`; the heap's `scope`, `BrandedBlock`, `BrandedCell`, `BrandedBox`, and `BrandedVec` now use melinoe's `InvariantLifetime<'brand>` marker and `ThreadLocalToken<'brand>` (minted by melinoe's `thread_local_scope`). Public renames: `AllocatorToken` → `ThreadLocalToken`, `Invariant` → `InvariantLifetime` (re-exported from `mnemosyne` and `mnemosyne-heap`). Both `ThreadLocalToken` (`PhantomData<*const ()>`) and the former `AllocatorToken` (`PhantomData<*mut ()>`) are `!Send + !Sync`, so all thread-confinement and brand-uniqueness proofs are preserved — verified by the unchanged `compile_fail` doctests (token `!Send`, `BrandedBox`/`BrandedVec` `!Send`, brand cannot escape its scope, cross-scope tokens/heaps cannot mix) plus the full heap unit/integration suite.
- The `melinoe` dependency tracks `main` with no pinned rev (`branch = "main"`, `default-features = false`), so the heap always builds against the latest published brand semantics.
- `BrandedBlock::cast<U>` is now an `unsafe fn`: it was callable from safe code
  to transmute-and-drop (`alloc_init(0usize)` → `.cast::<String>()` → `free`
  runs `String::drop` over integer bytes). `BrandedCell<'brand, T>` is now
  invariant in `T` (it is `Copy` with token-mediated interior mutability, so
  covariance allowed a safe dangling-reference exploit; a `compile_fail`
  doctest pins the rejection).
- Removed the dead `mnemosyne_core::sync::SpinLock` (no user remained after the
  huge-pool Treiber-stack conversion; its safe `unlock()` was callable without
  holding the lock).
- Removed the no-op `parallel`/`mnemosyne-memory` marker features from every
  leaf crate (they gated zero `cfg` sites); the top-level
  `mnemosyne/mnemosyne-memory → branded` forwarding remains. The permanent
  process-wide CUDA vectored-exception handler that converted in-page errors
  and nvcuda access violations into a silent `ExitProcess(0)` is replaced by a
  probe-window-gated handler that records the failure and terminates only the
  probing worker thread; `stagger_nextest_init` (test-runner detection on the
  production init path) is deleted.

### Migration

- Replace references to `mnemosyne::AllocatorToken` / `mnemosyne_heap::AllocatorToken` with `ThreadLocalToken`, and `Invariant` with `InvariantLifetime`. Code that obtains its token from the `branded_scope`/`scope` closure parameter needs no change (the token type is inferred).
- Wrap `BrandedBlock::cast` call sites in `unsafe` and discharge the documented
  layout + initialization/drop contract (freshly-allocated uninitialized
  storage written before use satisfies it, as `Heap::alloc_init` does).

### Changed

- Consolidation cycle 2 (redundancy-free SSOT pass): one
  `mnemosyne_core::abort::abort_on_corruption` for all corruption aborts;
  `Block::get_next/set_next` forward to their `_dynamic` twins;
  `Page::parent_segment`/`Segment::cookie_for`/`locate_segment` centralize the
  ~12 segment/cookie recovery copies; one `current_thread_id()` for the
  `gs:[0x48]` TEB read (was in three crates); `commit_in_place_free` +
  `do_local_free_internal` delegation collapse the duplicated free-transition
  arms; one `move_page_between_lists_branded`; `small_realloc_fits_existing_class`
  routes through `size_class::round_up_size`; `SecurePolicy`/`HardenedPolicy`
  fold into `mnemosyne-core::policy` (SSOT with `StandardPolicy`, with
  `mnemosyne-hardened` a thin re-export); `HasSegmentPool` reduces to one
  required `pools() -> &BackendPools` with default accessors, collapsing six
  copy-pasted per-backend blocks; `core/types/page.rs` splits into
  `page/{occupancy,init,reclaim}` leaf modules; benchmark bodies deduplicate to
  one generic `bench_*_case<A>` with a single `GATE_ROWS` threshold table.
- Cross-thread reclaim count accumulates in a per-`ThreadAllocator` field
  folded into the global atomic on `Drop`, removing a process-global
  `fetch_add` from the allocation-side reclaim path (cache-line ping-pong under
  producer/consumer loads).
- Huge-allocation cache behavior: the upward bucket scan stops once a bucket's
  lower bound exceeds `HUGE_POP_FIT_CAP (4) ×` the request, bounding cache-hit
  over-provision to <8× (a 20 KiB-class request can no longer pin a 16 MiB
  cached mapping whose RSS stays committed); the bucket count is derived from
  `MAX_CACHED_HUGE_SIZE` (11, was 16 — buckets 11–15 were unreachable dead
  statics scanned on every miss); exact-bucket fit misses restore the rejected
  chain with one splice CAS in original order instead of one CAS per segment.
- `ArenaMemoryStats` reports the enforced runtime retained-segment cap (not the
  compile-time limit) and gains `retained_huge_blocks`/`retained_huge_bytes`;
  `mnemosyne::MemoryStats` and the benchmark memory report forward them.
- The CUDA backend is a directory module (`backends/cuda/{loader, veh,
  registry, context, mod}.rs`): one cached library loader (temp-context calls
  no longer re-dlopen), one symbol resolver, and one monomorphized driver over
  a ZST `CudaAllocOps` strategy replacing three cloned `MemoryBackend` impls.
- Workspace profiles are pinned (dev/test `line-tables-only` + overflow
  checks, release stripped, a dedicated `profiling` profile) and
  `.config/nextest.toml` commits the 30 s slow / 60 s terminate test budget.
- `decay_step` no longer sweeps `DefaultBackend` (its pools are populated only
  by test fixtures; the swept-backend list now documents its closed-set
  maintenance hazard).
- Restored the no-argument tagged-stack head constructor used by
  `TaggedSegmentStack` and routed huge-pool rejected-chain restoration through
  `TaggedSegmentStack::push_chain`, making the batch CAS path production-live
  instead of test-only. Evidence tier: compile-time validation plus downstream
  Kwavers FWI integration; arena fmt/check/clippy pass, and downstream Kwavers
  FWI nextest passes.
- Consolidated `BrandedVec` shrink mechanics into a private `shrink_to_len`
  helper shared by `shrink_to_fit` and `into_boxed_slice`. The boxed-slice
  conversion still owns slice construction and ownership transfer; only the
  free-empty/realloc-to-length allocation transition is shared.
- Centralized arena NUMA bucket stealing in `segment/pool/numa_bucket.rs`.
  The huge pool and segment pool now share one `NUMA_BUCKETS` constant, one
  Themis-backed bucket conversion, and one generic nonlocal steal traversal.
  Evidence tier: value-semantic traversal tests plus `mnemosyne-arena` package
  gates; no benchmark speedup is claimed.
- Reduced `mnemosyne-prof` leak/dump memory pressure and contention. Live
  samples now store fixed-width `StackId` handles instead of owned
  `Box<[usize]>` stacks; a refcounted `StackInterner` stores one `Arc<[usize]>`
  per distinct live call stack, releases it on the last free, and recycles id
  slots. Stack capture uses a fixed stack buffer, repeat call sites avoid boxed
  frame allocation, and dump paths release shard mutexes before symbolication
  and file I/O. Evidence tier: value-semantic interner and snapshot unit tests
  plus package clippy, nextest, doctest, rustdoc, and stable/nightly-TLS compile
  checks.
- Fixed the hidden `mnemosyne-prof/nightly_tls` allocation fast path by routing
  `on_alloc` through `tls::should_skip_alloc_fast_path` instead of reaching into
  `tls.rs`'s private `THREAD_STATE`. The TLS module now owns the reentrancy and
  sample-budget check for both the nightly `#[thread_local]` backend and the
  stable TLS backend, and shared `sample_debit` arithmetic saturates oversized
  allocation sizes before signed budget subtraction. Evidence tier: `rustup run
  nightly cargo check -p mnemosyne-prof --features nightly_tls`, package clippy,
  nextest, and rustdoc.
- Split `mnemosyne-backend` by concern into five sibling leaf modules: [`mapping`](crates/mnemosyne-backend/src/mapping.rs) owns the `MemoryBackendWrapper` shape and central `impl MemoryBackend` block, [`guard`](crates/mnemosyne-backend/src/guard.rs) owns guard-region installation, [`reset`](crates/mnemosyne-backend/src/reset.rs) owns page-reset/decommit helpers, [`recorders`](crates/mnemosyne-backend/src/recorders.rs) owns telemetry counters and snapshots, and [`backends`](crates/mnemosyne-backend/src/backends/mod.rs) owns the per-OS / per-platform implementations. Public re-exports keep the canonical `mnemosyne_backend::*` import paths unchanged. Evidence tier: source-level static dispatch plus the backend unit suite and allocator benchmark threshold gate.
- Added the opt-in `mnemosyne-local/dealloc-probe` feature for deallocation branch-mix auditing. Default builds compile out the probe; feature builds expose `dealloc_counters::{reset, record, snapshot, total}` and record one Relaxed atomic increment at each committed `thread_free` arm. The integration test drives real `thread_alloc` / `thread_free_layout` calls and asserts layout-proven same-owner small frees all record as `InPlaceSmall` with zero huge-classifier or cold-path hits. Evidence tier: value-semantic integration test plus feature-gated unit tests.
- Extended the benchmark-summary enforcement set to include five realloc latency rows (`within_class_24_to_32`, `cross_class_32_to_64`, `within_class_6k_to_8k`, `cross_class_8k_to_16k`, `huge_shrink_4m_to_2m`) so future allocator changes cannot regress those paths outside the selected gate. Evidence tier: benchmark-summary config tests plus threshold-gate execution.
- Continued unsafe-discipline closure in `mnemosyne-core` and `mnemosyne-local` by documenting the `Segment` `Send`/`Sync`, Windows TEB thread-id read, local-free unchecked pointer/cookie updates, and native/ASM TLS allocator-pointer dereference invariants. No behavior change; evidence tier: clippy, nextest, doctests, and rustdoc.
- `purge_segment_pool`/`reset_segment_pool` now detach each NUMA node's retained
  chain under a single lock (new `NodeSegmentPool::take_all`) and run the OS
  release/reset off-lock, instead of one lock acquire/release per segment. With
  up to `MAX_RETAINED_SEGMENTS_LIMIT` (1024) segments this replaces up to ~1024
  lock round-trips on the per-node spinlock with one per node, so the decay
  thread no longer serializes round-by-round with allocators on the segment-refill
  hot path (mirrors `GlobalHugePool::purge`). Telemetry (`purge_calls`/`purged`/
  `reset_*`) is unchanged. Covered by a `take_all` value-semantic test.
- `GlobalSegmentPool::new`/`GlobalHugePool::new` build their node arrays from the
  `NUMA_BUCKETS` SSOT (`[const { Node*Pool::new() }; NUMA_BUCKETS]`) instead of a
  hand-written 16-element literal, so the fan-out can never drift from the constant.
- Huge-pool retention is now bounded by bytes per bucket, not just block count.
  A flat `MAX_CACHED_HUGE_BLOCKS` (1024) count cap let a large-size bucket retain
  up to ~16 GiB of idle mappings (1024 × 16 MiB); `bucket_block_cap` now caps each
  bucket at `min(MAX_CACHED_HUGE_BLOCKS, MAX_CACHED_HUGE_BYTES_PER_BUCKET / max
  block size)`, bounding any one bucket to ~256 MiB while leaving the small-huge
  buckets at the full count cap (no warm-cache regression). Verified by a
  value-semantic test on the per-bucket budget invariant.
- `GlobalHugePool::pop` no longer pre-loads each node's `total_count` before
  calling `pop_from_node` (which already early-returns on an empty node), removing
  one redundant atomic load per huge pop on the hit path and per probed node on
  the steal path.
- Small allocations requiring alignment above `MIN_BLOCK_SIZE` (16) now use the
  small thread-cache path instead of always falling back to the large/huge path.
  `thread_alloc_checked` rounds the request up to a multiple of `align` and
  accepts a size class whose block stride carries the alignment
  (`class_to_size(c) % align == 0`); page starts are `PAGE_SIZE`-aligned and
  blocks are carved at `block_size` stride, so this guarantees every block is
  `align`-aligned. Non-power-of-two-stride classes (48/80/96/…) still fall
  through to the large/huge path, preserving correctness. Previously every
  `align > 16` request — e.g. 64-byte-aligned SIMD buffers — took the
  ~2 MiB-per-allocation huge path regardless of size; a downstream consumer
  measured 512 live 256-byte/64-aligned allocations dropping from ~1056 MiB to
  ~4 MiB mapped. The alloc, free, and realloc paths now share one routing
  decision (`small_path_class`, SSOT), so they can never disagree on whether a
  block is small (a disagreement would be UB): the free fast path
  (`LAYOUT_PROVES_SMALL`) and the in-place realloc target class are both derived
  from it, and realloc no longer picks a class whose stride cannot carry the
  alignment. Verified by value-semantic tests asserting returned-pointer
  alignment and usability across alignments {16,32,64,128,256} (including the
  non-power-of-two classes) and an aligned-realloc-grow alignment-preservation
  test.
- Large/huge allocation fallback paths in `mnemosyne-local` now share one
  helper that performs the real allocation and policy-selected byte
  initialization.
- Per-CPU cache allocation/free retries now perform at most one CPU-id refresh
  probe per attempt after CAS contention.
- Page-local small-allocation paths now share one helper for local free-list
  pop or lazy bump allocation plus allocation-count accounting.
- Orphan segment adoption now uses the guarded segment-aware page remote-free
  reclaim helper, avoiding atomic drains on empty page-local remote-free queues
  during adopted-segment scans.
- Allocation-side remote-free recovery now checks for an empty page-local
  queue inside `try_reclaim_and_allocate`, so active and full page recovery
  paths share one guarded drain helper.
- Allocator segment sweeps now share
  `Page::reclaim_thread_free_if_present_for_segment` for guarded remote-free
  reclamation, keeping the empty-queue fast path in one page-level API.
- Thread-exit owned-segment reclamation now checks page-local remote-free
  queues before attempting an atomic drain while still scanning every page's
  live allocation count.
- Periodic allocator defragmentation now checks page-local remote-free queues
  before attempting an atomic drain, matching targeted segment reclaim behavior
  while preserving occupied-page accounting.
- Allocator segment reclaim and defragmentation sweeps now call a
  segment-aware page cross-thread-free reclaim helper, reusing the known
  `Segment` pointer and page index instead of re-deriving them from the page
  address.
- `benchmark_summary` now parses summary CSV rows through a lending `Cow`
  iterator instead of collecting fields into a `Vec` and cloning the benchmark
  name.
- `benchmark_summary` now builds missing selected-row diagnostics directly
  from the baseline iterator instead of collecting missing benchmark names into
  a `Vec`.
- `benchmark_summary` now streams baseline comparison rows through a lending
  iterator instead of collecting `ComparisonRow` values and cloning benchmark
  names before CSV output and threshold checks.
- `benchmark_summary` now streams selected baseline excerpt rows directly to
  CSV writers instead of collecting them into an intermediate `Vec`.
- Split `mnemosyne-prof` TLS provider selection and per-thread hook state into
  `src/tls.rs`. Public profiler controls and allocation/free hook entry points
  remain in `src/lib.rs`; behavior and exported API are unchanged.
- Split the `mnemosyne` global allocator integration tests into basic
  allocation, stats/cache, realloc, policy/backend, and leak-detector leaf
  modules. Runtime behavior is unchanged; the integration root now owns only
  the global allocator declaration and shared imports.
- `mnemosyne-local` now uses `melinoe::thread_cached!` for the allocator TLS
  seed cache, preserving the nonzero randomized seed contract while removing
  duplicate local stable/nightly cache branches.
- Split `mnemosyne-heap` unit tests into heap, boxed, cell, vector, and trait
  leaf modules. Runtime behavior is unchanged; the root test module now owns
  only shared fixtures and module wiring.
- Removed `benchmark_summary`'s command-argument `Vec` allocation; known flags
  are parsed in one pass while preserving unknown-flag tolerance.
- Refreshed `benchmarks/allocator_comparison.md` from the current
  `system-jemalloc` Criterion benchmark matrix. The selected-row threshold gate
  passes after a focused segment-cache eviction rerun stabilized the only
  initial alert.
- Split the `benchmark_summary` binary into dedicated config, CSV, Criterion,
  report, allocator-rendering, metadata, and threshold modules. The entrypoint
  now only orchestrates the report pipeline, and the largest new leaf module is
  below the 500-line structural target.
- Added stable `std_tls` feature routing for allocator/profiler TLS selection
  and re-exported `MemoryBackendWrapper`/`LocalAllocatorSelector` through the
  top-level `mnemosyne` crate so consumers can name the monomorphized backend
  selector surface directly.
- Replaced platform-local current-CPU probes in `mnemosyne-local` with
  `themis::current_processor()`, making Themis the single source of topology
  identity for per-CPU allocator cache routing.
- Added `ScratchBank<T, const N>` to the scratch provider surface so transform crates can keep fixed role-specific scratch pools in one const-generic bank instead of duplicating per-role thread-local pool declarations. Verified by slot-independence scratch tests, `cargo check -p mnemosyne-arena`, clippy, and docs.
- Routed Rust `GlobalAlloc::dealloc` through a layout-aware small-free entry point that removes the large/huge classifier branch when the original `Layout` proves a small allocation; pointer-only `thread_free` remains the classifier-backed API for unknown-layout callers.
- Fixed the combined usable-size benchmark harness so the fresh allocation pointer is consumed through `black_box` before `usable_size` and before `dealloc`. The prior helper let LLVM cross-optimize the allocation/query/free sequence and produced an inverted Mnemosyne row (`large_8192` faster than small/medium). Focused Criterion now reports `small/32` and `medium/1024` near `2.3 ns`, with `large/8192` near `5.2 ns`.
- Added active RpMalloc benchmark coverage and reduced full-page local-free transition overhead by storing the proved owner allocator cache pointer on segments, avoiding redundant busy-bit writes for first frees from full pages, and moving full pages back to active pages with one branded list-token operation.
- Updated the `melinoe` lockfile resolution to `85d498bb` (`0.5.0`) and kept `mnemosyne-heap` as the single branded-token consumer.
- Made top-level branded heap re-exports an optional default `mnemosyne/branded` feature. Default users keep the same API; allocator-only builds can disable default features to avoid linking unused branded heap machinery.
- Replaced runtime size-class leading-zero arithmetic with a compile-time-generated `u8` lookup table for every small allocation size.
- Removed per-row allocator-name `Vec`, owned comparison-key, formatted-cell, lowercase, and Criterion path-normalization allocations from `benchmark_summary` by using borrowed slices, copy-sized display adapters, and direct writes into existing output buffers.
- Removed profiler dump snapshot clones and intermediate symbol vectors: `dump_profile` and `dump_leaks` now borrow active samples shard-by-shard, reuse exact retained stack slices, and stream report output directly.
- Converted the huge-allocation cache's `NodeHugeBucket` from a
  spinlock-protected intrusive list to a lock-free Treiber stack. The exact
  same-size bucket still finds a fitting segment behind undersized heads by
  restoring temporarily rejected segments, while higher buckets pop the head
  directly. `NodeSegmentPool` and `NodeHugeBucket` now share cache-line-aligned
  atomic wrappers from `segment/pool/cache_aligned.rs`; the huge-pool head uses
  a tagged pointer on 64-bit targets to prevent stale-head ABA under concurrent
  pop/push stress.

### Fixed

- Made `benchmark_summary` report and metadata writers create missing parent
  directories before opening output files, so a clean checkout without
  `target/criterion` reports selected-row absence instead of failing at file
  creation.

### Removed

- Removed tracked unreferenced scratch artifacts `scratch/test.cxx` and
  `scratch/test.exe`.

## 0.1.0

### Fixed

- Retained the active thread-local segment during local frees to avoid immediate segment recycling on hot allocate/free cycles.
- Fixed Unix backend constant typing so formatting can parse every target module.
- Bounded the global free segment cache and released excess empty segment mappings to the OS.
- Avoided segment-reclaim calls on hot local frees for the current thread-local segment.

### Fixed

- Fixed a debug-build underflow panic in `is_valid_alloc_request` / `is_valid_layout_alloc_request`: a branchless `(size - 1) < MAX_ALLOC_SIZE` formulation panics on `size == 0` under debug overflow checks (correct only in release). Every allocation validates size first, so a zero-size request panicked mid-allocation and corrupted the process-wide segment pool, cascading into unrelated test failures. Replaced with `size.wrapping_sub(1) < MAX_ALLOC_SIZE` (branchless, panic-free, semantically identical). Guarded by the zero-size assertions in both validator tests.
- Fixed a latent alignment UB in `mnemosyne-core::types::test_page_reclaim_thread_free` (surfaced by Miri): the test backed a page with a 1-byte-aligned `[u8; PAGE_SIZE]` array and wrote 8-byte-aligned `Block` pointers through it. Backed the storage with a `#[repr(align(64))]` wrapper. Production is unaffected (real page starts are `PAGE_SIZE`-aligned). The re-entrancy fix and doubly-linked owned-segment splice tests also pass under Miri with no UB.
- Closed a latent re-entrancy soundness hole on the guard-free small-allocation fast path: it borrowed the thread cache without checking the `is_allocating` busy bit, so a same-thread re-entrant allocation could create a second aliasing `&mut ThreadAllocator` (undefined behavior). The fast path now pops through the new `with_allocator_unguarded` primitive, which still consults the busy bit (returning `None` on re-entry) while skipping the guard set/clear writes. Pinned by `unguarded_fast_path_rejects_reentrant_borrow`.

### Changed

- Made `nightly_tls` compiler-channel-aware: stable builds, including `--all-features`, now compile the portable TLS path, while the unstable `#[thread_local]` path is enabled only when the active `RUSTC` is nightly.
- Consolidated heap APIs to one scoped `mnemosyne_heap::Heap<'brand, P, B>` backed by the single internal `RawHeap<P, B>` implementation. Removed the duplicate `MnemosyneHeap` and `BrandedHeap` public wrapper identities and updated tests/profiler coverage to use scoped branded ownership.
- Removed `MnemosyneHeap` and `BrandedHeap` from allocator comparison reporting. The benchmark matrix now compares the canonical Mnemosyne allocator against external allocator backends, and SnMalloc `huge_2m` rows are measured instead of reported as `N/A`.
- Added a value-semantic integration test suite (`mnemosyne-local/tests/hot_path_value_semantics.rs`) hammering the allocate/free/usable_size hot paths: distinct/non-overlapping/round-trip checks across every size class, allocate-free churn that drives page recycling and segment reclaim, and a zero-size-returns-null guard. The distinct/round-trip check catches corruption-class regressions (overlapping or wrong-class blocks from unchecked-indexing or size-mapping optimizations) that pass per-operation but fail end-to-end. Runs under `cargo test` (real backend), complementing the Miri-validated pure-logic unit tests.
- Completed the per-component complexity review in `complexity_audit.md`: added C-ABI-shim and backend operation tables (all entry points O(1) plus the O(n) inherent work of `calloc` zeroing and cross-class `realloc` copy) and recorded the landed `Page::index_in_segment()` O(1) derivation foundation.
- Removed a dead `min` branch in the C shim's `realloc`: after the `new_size <= current_usable` early return, the copy length is provably `current_usable`, so the branch was unreachable. No behavioral change (pinned by `realloc_preserves_bytes_across_grow`).
- Enabled the jemalloc benchmark comparator on Windows via an opt-in `system-jemalloc` feature. `tikv-jemallocator` builds jemalloc from source, which does not link on windows-gnu (the jemalloc column was N/A); the feature instead links a system-installed static `libjemalloc_s.a` (e.g. MSYS2 `mingw-w64-*-jemalloc`) through a thin `GlobalAlloc` over jemalloc's sized `je_*x` API, with `build.rs` locating the lib from `PATH` (`*/{ucrt64,mingw64}/bin` siblings) or `MNEMOSYNE_JEMALLOC_LIB_DIR`. Default Windows builds are unchanged (feature off ⇒ jemalloc still skipped); non-Windows targets keep `tikv-jemallocator`. Run with `cargo bench -p mnemosyne-benchmarks --features system-jemalloc`. Verified: links and produces real numbers on windows-gnu (e.g. cycle latency small/medium/large ≈ 6.98 / 7.49 / 15.30 ns).
- Reduced Windows commit charge by ~`SEGMENT_ALIGN` (≈ 2 MiB) per segment: added `MemoryBackend::decommit` (Windows `VirtualFree(MEM_DECOMMIT)` / Unix `madvise(MADV_DONTNEED)`) and used it in `allocate_segment` to return the eagerly-committed alignment slack `[raw_ptr, aligned_addr)` to the OS while keeping the reservation releasable. Unlike `page_reset` (`MEM_RESET`), `decommit` actually drops commit charge. Records `decommit_calls`/`decommit_bytes` telemetry. Backends without it opt out via the default. Pinned by `decommit_telemetry_increments_call_and_byte_counters_only` and `wrapper_decommit_returns_slack_and_keeps_reservation_releasable`.
- Added `Page::index_in_segment()`, an O(1) address-derivation of a page's index within its segment, validated against the stored `page_index` field across a real segment (`page_index_field_matches_address_derivation`). This is the verified foundation for replacing the stored `page_index` with a doubly-linked `prev_page` back-pointer (O(1) page-list unlink) while keeping `Page` within its 64-byte cache line.
- Hardened the `AtomicFreeList` 64-bit pointer-packing deallocation queue: replaced bare integer/pointer casts with explicit `expose_provenance`/`with_exposed_provenance_mut` (provenance-correct for a tagged-pointer list), replaced magic-number masks with named `PACKED_PTR_BITS`/`PTR_MASK`/`COUNT_WRAP_MASK` constants, and documented the 48-bit address and 16-bit counter portability contract. No behavior or codegen change; validated under Miri (no UB).
- Added a Miri-validated pure-logic test for the singly-linked page-list splice helper (`unlink_page_from_list`), which previously had no Miri-runnable coverage.
- Reduced `unlink_owned_segment` from O(owned-segments) to O(1) by converting the owned-segments list to an intrusive doubly-linked list (`Segment::prev_owned_segment`), routing both insertion sites through the single authoritative `ThreadAllocator::push_owned_segment`. Removes the owned-segment-count term from `try_reclaim_segment`. Pinned by `owned_segment_list_is_doubly_linked_and_unlinks_in_place`.
- Added `complexity_audit.md`, a per-component asymptotic-complexity review confirming all per-allocation/per-free hot paths are O(1) and cataloguing the remaining cold-path super-constant operations with a reduction plan.
- Added an optional `nightly_tls` feature to `mnemosyne-local` that replaces the portable `std::thread_local!` cache accessor with an ELF/PE `#[thread_local]` static. The fast accessor lowers to a single segment-register-relative load with no `LocalKey::with` call or lazy-initialization guard — the mechanism mimalloc uses for its default heap — targeting the single-threaded small-allocation cycle-latency gap. The default build remains on stable Rust and is byte-identical; the feature requires a nightly compiler.
- Preserved thread-exit segment reclamation on the `#[thread_local]` fast path via a `std::thread_local!` `Drop` sentinel (`ThreadExitReclaim`), because `#[thread_local]` statics are not dropped on thread teardown. Reclamation logic is shared with the default `Drop` path through `ThreadAllocator::reclaim_owned_segments`, which now clears the owned-segment head to stay idempotent.
- Added a value-semantic nightly-only test proving the exit sentinel orphans a terminating thread's still-live owned segment.
- Expanded allocator benchmarks to compare Mnemosyne, mimalloc, and snmalloc across cycle latency, burst retention, and threaded small-allocation cycles.
- Added cross-thread free handoff benchmarks for Mnemosyne, mimalloc, and snmalloc.
- Added Mnemosyne memory telemetry for backend mappings and retained arena segments.
- Added current-thread live allocation, current-thread owned segment, and cross-thread reclaimed block telemetry.
- Added a release-mode `memory_report` CSV command for telemetry inspection.
- Replaced cross-thread free benchmark thread creation with persistent bounded-channel workers.
- Added per-size-class occupancy telemetry and report rows.
- Replaced threaded allocation benchmark thread creation with persistent bounded-channel workers.
- Added deterministic segment-cache eviction benchmark coverage and an `eviction_after` memory report row.
- Added arena purge telemetry for purged segment count, purge call count, and purged bytes.
- Added a `benchmark_summary` release command that extracts compact Criterion mean/median CSV rows.
- Added a `purge_after` memory report row proving retained segment cache purge behavior.
- Added a source-controlled selected Mnemosyne benchmark baseline excerpt.
- Added benchmark baseline metadata for platform, toolchain, and benchmark commands.
- Added current-to-baseline benchmark comparison CSV generation for selected Mnemosyne rows.
- Fixed `thread_free` segment metadata scope so small-allocation page-owner logic compiles after classification.
- Added value-semantic tests for benchmark summary CSV parsing and ratio computation.
- Fixed the memory retention-bound test assertion syntax.
- Stabilized the page-recycling test by asserting segment reuse and target size-class metadata.
- Changed `benchmark_summary` so source-controlled baseline refresh requires `--refresh-baseline`.
- Routed cross-thread small frees through page-local atomic free lists.
- Removed duplicate small-free segment metadata derivation.
- Preserved the hot local allocation path by batch-reclaiming page-local remote frees after local free blocks are exhausted.
- Centralized page-local cross-thread free reclamation in an inlined `Page::reclaim_thread_free` method.
- Added direct value-semantic tests for page-local remote-free reclamation.
- Bound global allocator routing to the zero-sized `StandardPolicy` allocation policy.
- Removed the panic-bearing `align_up` API and retained `checked_align_up` as the production alignment contract.
- Replaced hot-path `expect`/`unwrap` calls in `ThreadAllocator::alloc`, `alloc_cold`, `get_new_page`, and `try_recycle_page` with `debug_assert!` plus `core::hint::unreachable_unchecked` for verified structural invariants.
- Dropped the stale `align_up` re-export from `mnemosyne-arena::lib` to restore a clean workspace build.
- Changed benchmark threshold gating so quick-mode summary extraction fails only when `--enforce-thresholds` is passed.
- Moved generated benchmark metadata to `target/criterion/benchmark_metadata.json`.
- Stabilized page-recycling test assertions under reusable segment state.
- Removed benchmark-summary test-build dead-code warning for the metadata output path.
- Centralized allocation initialization and free poisoning through inlined `AllocPolicy` helper functions so standard and secure policy routes remain monomorphized while avoiding duplicated unsafe write blocks.
- Serialized allocator integration tests that mutate global segment-pool state so purge telemetry assertions remain deterministic under the default parallel test harness.
- Replaced raw segment owner pointers with a transparent `SegmentOwner` permission token, separating ownership identity from segment metadata while preserving pointer-sized layout and comparison cost.
- Removed the allocator-level incoming free queue and routed re-entrant local frees through the existing page-local atomic queue, reducing `ThreadAllocator` state and eliminating the remaining owner-token cast.
- Added value-semantic coverage proving re-entrant local frees enqueue to the page-local atomic queue and reclaim back into the page free list.
- Completed backend-specific segment-pool typing through `HasSegmentPool`, including backend-typed arena telemetry and benchmark call sites.
- Added a saturated threaded small-allocation benchmark group that keeps the same worker topology while increasing per-command allocation work to isolate allocator throughput from bounded-channel coordination overhead.
- Fixed backend-specific `LocalAllocatorSelector` generation so each backend implementation owns distinct thread-local allocator and re-entrancy state.
- Added page-refill telemetry and deferred owned-segment recycle sweeps until the current segment has no unsliced pages, reducing unnecessary cold-path scans during fresh page refills.
- Replaced the scheduler-sensitive historical threaded baseline gate with the saturated threaded benchmark row while keeping the historical row in allocator comparison reports.
- Replaced benchmark-runner panic assertions and unwrap/expect calls with explicit benchmark failure diagnostics.
- Documented safety contracts for benchmark unsafe operations and allocator policy byte-initialization helpers.
- Made CUDA unified-memory dynamic initialization race-free with a three-state gate, bounded registry tests, and host fallback when registry capacity is exhausted.
- Updated README architecture notes for page-local remote-free routing and CUDA fallback semantics.
- Replaced backend peak-mapping telemetry compare-exchange loop with `fetch_max`, documented relaxed counter semantics, and added value-semantic telemetry tests.
- Changed `MemoryBackend::deallocate` to return a release-success boolean; `MemoryBackendWrapper` now defers `current_mapped_bytes` decrements to confirmed OS release and routes failures through a `record_unmap_failure` path that only increments the call counter. Propagated through unix, windows, CUDA, and the `mnemosyne-local` test backend.
- Changed arena purge accounting to count only confirmed backend releases and retain unreleased segments in the pool when release fails.
- Marked `MemoryBackend::deallocate` as `#[must_use]`, propagated huge-allocation release status through `deallocate_large_or_huge`, and retained full-pool segment mappings when direct backend release fails.
- Documented the huge-allocation user-pointer/metadata-slot layout derivation, validated power-of-two alignments before backend allocation, added `debug_assert!` checks for pointer alignment, reserved-prefix containment, and payload mapping bounds, and pinned the contract with alignment-grid and invalid-alignment tests.
- Rejected huge allocation alignments above `SEGMENT_SIZE` to preserve zero-copy free classification by segment rounding or metadata-slot lookup without adding a side registry.
- Rejected invalid direct `thread_alloc` alignments before size-class or arena routing, and made `allocate_large_or_huge` reject zero alignment.
- Rejected zero-size allocation requests at global, local, and arena allocation entry points.
- Documented the small-free classifier invariant in `thread_free` and added `debug_assert!` checks for `page_index < PAGES_PER_SEGMENT`, `page.block_size > 0`, and block-stride aligned offset; pinned the contract with `small_alloc_returns_block_aligned_ptr_outside_metadata_page` across 8 B–1 KiB requests.
- Added `MAX_ALLOC_SIZE` and rejected direct allocation requests whose payload or backend mapping requirement exceeds the pointer-offset-safe allocation bound.
- Split `thread_alloc_layout` from direct `thread_alloc` so Rust `Layout`-validated global allocator calls keep a monomorphized hot path without repeated power-of-two validation.
- Released live allocations and serialized shared-state local allocator integration tests so benchmark and unit verification run against deterministic allocator state.
- Extracted `is_valid_alloc_request` and `is_valid_layout_alloc_request` `const fn` predicates in `mnemosyne-core::validation`, replacing per-clause `size`/`align` checks in `thread_alloc`, `thread_alloc_layout`, and `allocate_large_or_huge` with a single zero-cost validation surface.
- Reduced the huge-allocation backend mapping size to `size + alignment + SEGMENT_ALIGN + PAGE_SIZE`, dropping the previous `2 * SEGMENT_SIZE` slack reservation that wasted ~2 MiB − 64 KiB per huge mapping. Pinned by `huge_allocation_consumes_tight_mapping_size` which asserts the exact mapped-byte delta via backend telemetry.
- Removed the dead `Page::segment` back-pointer field and unused `Page::is_empty` helper, shrinking `Page` from 72 to 64 bytes (one cache line on 64-bit targets), saving 256 bytes per segment metadata and 32 stores per fresh segment initialization. Pinned by `page_struct_size_stays_within_one_cache_line`.
- Documented the source-controlled baseline versus generated `target/criterion` artifact boundary in `allocator_baseline_metadata.md` and `allocator_comparison.md`, recording which local-allocator optimizations the baseline pre-dates.
- Augmented bare `assert!(expr)` invocations in workspace tests with value-semantic diagnostic messages that name the violating operand or telemetry counter on failure; no assertion was relaxed.
- Replaced bare test `unwrap()` calls with contextual `expect()` diagnostics and serialized arena allocation telemetry tests so exact mapped-byte assertions are deterministic under the parallel test harness.
- Documented the `size_to_class(0)` zero-size mapping contract and added boundary-walking coverage that verifies every consecutive size-class transition (including the four piecewise jump points at 128/129, 512/513, 2048/2049, and 8192/8193) is exact.
- Extracted `try_reclaim_and_allocate` `#[inline(always)]` helper that folds the three "drain `thread_free` → record telemetry → pop free block → bump alloc_count" sites in `ThreadAllocator::alloc` and `alloc_cold` into a single routine; hot-path codegen is preserved by monomorphization.
- Added value-semantic messages to production `debug_assert!` invariant checks while preserving release-mode zero-cost behavior.
- Refreshed selected Mnemosyne threshold-gated Criterion rows after the remote-free helper refactor; full broad Criterion filters are tracked for follow-up because they exceeded the local command cap.
- Replaced the unsupported `--quick` benchmark invocation with an explicit bounded Criterion smoke configuration in `allocator_bench`.
- Added eleven compile-time `const _: () = assert!(...)` items across `mnemosyne-core::constants` and `mnemosyne-core::size_class` pinning power-of-two layout sizes, `SEGMENT_ALIGN`/`PAGE_ALIGN` agreement, `PAGES_PER_SEGMENT * PAGE_SIZE == SEGMENT_SIZE`, `MAX_SMALL_ALLOC_SIZE <= PAGE_SIZE`, `MAX_ALLOC_SIZE >= SEGMENT_SIZE`, `NUM_SIZE_CLASSES > 0`, and the size-class schedule endpoints. Constant drift now produces a compile error.
- Made local-free full-page reactivation conditional on `unlink_full_page` finding and removing the page from the full-list, preventing stale or already-unlinked pages from being inserted into the active-page list.
- Refreshed the source-controlled selected Mnemosyne benchmark baseline from the bounded Criterion smoke harness and synchronized the baseline metadata.
- Confirmed `thread_free` owner-token checks use `LocalAllocatorSelector::get_allocator_ptr`, avoiding an allocator-cell borrow for the pointer comparison before the local-free mutation path.
- Added jemalloc as a target-gated allocator benchmark comparator and extended `allocator_comparison.md` with Jemalloc columns. Windows GNU reports `N/A` for jemalloc rows because `tikv-jemallocator` requires a linkable native static jemalloc library on the active target.
- Extracted `unlink_page_from_list` `#[inline]` helper that consolidates three duplicated singly-linked page-list walks in `unlink_full_page` and `unlink_page`; the helper takes the list head slot by mutable reference, returns the found-status as a bool, and is inlined at every call site for unchanged release-mode codegen.
- Added Linux `MADV_HUGEPAGE` advisory hint in `UnixBackend::allocate` for mappings that are at least one full `SEGMENT_SIZE` (2 MiB) and a multiple thereof; lets the kernel back each 2 MiB segment with a single transparent huge page entry, halving TLB pressure on segment-metadata accesses. The advice is purely advisory and ignored on kernels without THP support. Non-Linux Unix targets compile a stub.
- Added `MemoryBackend::page_reset(ptr, size) -> bool` trait method with a default `false` impl. Implemented on `UnixBackend` via `MADV_DONTNEED` (Linux) and `MADV_FREE` (macOS/FreeBSD), and on `WindowsBackend` via `VirtualAlloc(MEM_RESET, PAGE_READWRITE)`. `MemoryBackendWrapper` records `page_reset_calls` and `page_reset_bytes` telemetry without touching `current_mapped_bytes` since the virtual mapping stays committed. Counters surface in `BackendMemoryStats` and `mnemosyne::MemoryStats`.
- Added `mnemosyne_arena::reset_segment_pool` and the public `mnemosyne::reset()` / `reset_generic<B>()` APIs that drop the physical backing of every retained free segment while keeping them cached for reuse. Records `reset_calls` and `reset_segments` on `GlobalSegmentPool`; counters surface in `ArenaMemoryStats` and `mnemosyne::MemoryStats`. Complements `purge()` as a lighter-weight RSS-reduction knob.
- Added `MemoryBackend::make_guard(ptr, size) -> bool` trait method with a default `false` impl. Implemented on `UnixBackend` via `mprotect(PROT_NONE)` and on `WindowsBackend` via `VirtualProtect(PAGE_NOACCESS)`. `MemoryBackendWrapper` records `guard_install_calls` and `guard_install_bytes` telemetry without touching `current_mapped_bytes` since the mapping stays reserved. Counters surface in `BackendMemoryStats` and `mnemosyne::MemoryStats`.
- Added an opt-in `mnemosyne-arena/segment-tail-guards` feature with `SEGMENT_TAIL_GUARD_SIZE = 4 KiB` (compile-time `is_power_of_two` and `<= SEGMENT_ALIGN` checks). When enabled, `allocate_segment` installs a `PROT_NONE` / `PAGE_NOACCESS` guard at `aligned_addr + SEGMENT_SIZE` on every fresh OS-backed segment. The default feature set leaves the install disabled so benchmarked allocator paths keep zero guard-install overhead.
- Extended `memory_report` with page-reset, guard-install, reset-segment, and reset-call telemetry columns plus a `reset_after` phase before purge.
- Stabilized the reset integration test around the real invariant: reset must not reduce retained segment count, while process-wide retained segments may increase when other thread-local allocators return segments.
- Marked `size_to_class` and `class_to_size` as `#[inline(always)]` so allocator hot paths receive cross-crate mapper bodies under monomorphization.
- Moved secure-policy small-free poisoning after small-page classification so poisoned frees reuse the classifier's page metadata lookup.
- Refreshed `allocator_comparison.md`; the current run reports Mnemosyne small cycle latency at `12.975 ns` and saturated threaded small cycles at `201.364 us`.
- Added `mnemosyne::usable_size(ptr)` (re-exported from `mnemosyne_local`) that returns the allocator's actual reservation for a previously-allocated pointer: the size-class block size for small allocations, the payload remainder for huge allocations, and 0 for null. Mirrors `mi_usable_size` / `malloc_usable_size` from mimalloc and glibc/jemalloc.
- Added `Usable size latency` Criterion coverage for Mnemosyne, mimalloc, snmalloc, and target-gated jemalloc, and included those rows in generated allocator comparison reports.
- Optimized `usable_size` small-allocation classification by reading the target page block size before the huge-allocation metadata fallback.
- Overrode `GlobalAlloc::realloc` on `Mnemosyne` and `MnemosyneAllocator<P, B>` to consult `usable_size(ptr)` and return the same pointer unchanged when the new size fits inside the existing size-class block. Secure policies keep replacement allocation on growth so new bytes are zero-initialized.
- Added `Realloc latency` Criterion coverage for within-class and cross-class realloc cycles across Mnemosyne, mimalloc, snmalloc, and target-gated jemalloc.
- Added `Usable size query latency` Criterion coverage that isolates raw usable-size metadata lookup from allocation/deallocation cost.
- Added `Allocator allocation latency` Criterion coverage with drop-guard cleanup to isolate allocation cost from deallocation cost.
- Added `std::alloc::System` allocator comparator rows and System ratio columns to generated benchmark reports for portable allocator-operation groups.
- Optimized `thread_free` by classifying small frees from the target page block size before the huge-allocation fallback and by deriving the owner-token comparison from the existing allocator TLS access.
- Added `Allocator deallocation latency` Criterion coverage that allocates during setup and measures only deallocation across Mnemosyne, System, mimalloc, snmalloc, and target-gated jemalloc.
- Removed the dead `Page::local_free` metadata field and its allocation fast-path branch; local frees already return blocks directly to `Page::free` and re-entrant/cross-thread frees use `Page::thread_free`.
- Added a small-realloc size-class proof fast path so standard-policy realloc returns the same pointer without a `usable_size` metadata query when the old `Layout` already proves the existing small size class covers the new request.
- Added a current-segment marker to segment metadata so same-thread frees on the active segment return blocks to the page free list without taking the allocator `RefCell` mutation path.
- Added a combined allocator guard selector so small allocations clear the TLS re-entrancy guard inside the allocator access path instead of performing a separate TLS lookup.
- Replaced the piecewise `size_to_class` hot-path arithmetic with a compile-time lookup table while preserving the zero-size mapping contract and existing boundary coverage.
- Replaced thread-local allocator `RefCell` access with guarded `UnsafeCell` access under the existing per-thread allocation flag, removing dynamic borrow checks from allocator hot paths while keeping re-entrant access routed through existing fallback paths.
- Added `benchmark_variance.csv` generation to `benchmark_summary`, recording Criterion mean confidence intervals, relative CI width, and unstable-row classification for variance-aware optimization decisions.
- Fixed `usable_size` over-report for huge allocations: now uses `segment.raw_alloc_ptr + huge_size - ptr` as the suffix length instead of `segment_ptr + huge_size - ptr` (where `segment_ptr = aligned_addr` could sit up to `SEGMENT_ALIGN - 1` bytes past the actual OS mapping boundary). The same fix applies to `thread_free`'s `SecurePolicy` poisoning size on the segment-aligned and fallback huge-allocation paths. Pinned by `usable_size_does_not_over_report_past_mapping_end_for_huge_allocations`.
- Extracted `Segment::huge_mapping_suffix_from(user_ptr) -> usize` helper that centralizes the `raw_alloc_ptr + huge_size - ptr` derivation. All four prior consumers (`usable_size` segment-aligned, `usable_size` fallback, `thread_free` `SecurePolicy` poison on both huge-allocation branches) route through the helper, making the over-report bug class structurally impossible to reintroduce.
- Documented realloc slow-path copy bounds: replacement realloc copies only the caller-initialized `min(layout.size(), new_size)` bytes, not allocator size-class slack.
- Documented the `realloc` slow-path copy-length contract on both `Mnemosyne` and `MnemosyneAllocator<P, B>`: copy is `min(layout.size(), new_size)` because bytes beyond `layout.size()` are size-class slack the user never initialized. Pinned by `test_realloc_does_not_copy_past_layout_size`.
- Collapsed the per-thread allocation guard and allocator cache into one `LocalAllocatorSlot<B>` TLS key. This preserves the guarded `UnsafeCell` aliasing contract while reducing small allocation and free owner-token lookup overhead. The refreshed Windows comparison reports Mnemosyne saturated threaded small cycles at `69.874 us`, versus mimalloc `64.662 us`, system `361.401 us`, and snmalloc `262.669 us`.
- Rejected forced `AtomicFreeList` inlining after measurement showed it traded a cross-thread handoff improvement for a saturated threaded cycle regression; the atomic queue keeps ordinary cross-crate inlining hints.
- Rejected `thread_local!` const initialization for the allocator slot after measurement showed the same saturated threaded regression pattern despite faster non-saturated rows.
- Added all-size-class `usable_size` lower-bound coverage and rejected separate owner-token TLS routing after measurement showed regressions in cycle latency, cross-thread handoff, and saturated threaded cycles.
- Added `usable_size_never_under_reports_across_every_size_class` exhaustive lower-bound test covering every small size class at its lower boundary and class max, bracketing `usable_size` from both sides alongside the existing over-report guard.
- Extracted `realloc_copy_grow<A: GlobalAlloc>` shared slow-path helper; `Mnemosyne::realloc` and `MnemosyneAllocator<P, B>::realloc` now route their allocate/copy/free round trip through one monomorphized function, consolidating the copy-length contract into a single rustdoc block.
- Marked the shared realloc slow-path helper `#[inline(always)]` after focused Criterion rows improved Mnemosyne realloc latency to `5.523 ns` for `within_class_24_to_32` and `12.932 ns` for `cross_class_32_to_64`.
- Rejected a <=128-byte direct realloc-capacity shortcut after it failed to beat the accepted within-class realloc point estimate and reported an allocator-cycle regression.
- Added the `mnemosyne-c-shim` crate exposing `malloc`/`free`/`calloc`/`realloc`/`aligned_alloc`/`posix_memalign`/`malloc_usable_size` as `extern "C"` (`lib` + `cdylib`), routing to the thread-local allocator under `StandardPolicy`. The cdylib can be `LD_PRELOAD`ed or DLL-injected to interpose the C allocator. The shim's `realloc` copies `min(usable_size, new_size)` to match C semantics (no tracked request size), distinct from the Rust path's `layout.size()` bound. Covered by 11 regression tests.
- Rejected deferred remote-free telemetry accounting and forced `Page::reclaim_thread_free` inlining after focused Criterion rows showed threaded or cross-thread regressions.
- Rejected forced `usable_size` inlining after focused Criterion rows regressed allocator cycle, combined usable-size, and raw usable-size query latency.
- Rejected a Layout-proven small-allocation entry split after it improved allocation-only small latency but regressed retained small allocation cycles and historical threaded small cycles.
- Serialized backend telemetry tests that mutate process-wide mapping counters, making the workspace test gate deterministic without changing production telemetry semantics.
- Added `crates/mnemosyne-c-shim/include/mnemosyne.h` C declaration header matching the seven exported shim symbols, with per-function contract documentation, so C/C++ consumers have a ready prototype file.
- Rejected compact `Page` counter layouts after 48-byte metadata experiments regressed saturated threaded cycles and usable-size latency.
- Centralized the 16-byte minimum small-block size as `MIN_BLOCK_SIZE` and routed size-class plus alignment-threshold logic through it.
- Added `smallest_class_page_saturates_without_duplicate_or_early_refill`, a runtime witness that a full 4096-block 16-byte page reaches `alloc_count == max_blocks` with distinct non-null pointers and refills cleanly past saturation.
- Removed the redundant `layout.size() == 0` guard from `Mnemosyne::alloc` and `MnemosyneAllocator::alloc`; `thread_alloc_layout` already rejects zero-size through `is_valid_layout_alloc_request`, so the GlobalAlloc hot path drops one branch and the zero-size contract lives in a single place.
- Rejected removing `MAX_ALLOC_SIZE` from the Layout-validated allocation predicate after focused benchmarks improved cycle and combined usable-size rows but regressed allocation-only and historical threaded small-allocation rows.
- Switched the `ALLOCATOR_SLOT` thread-local to the `const {}` initializer form so the compiler emits the const-init accessor (omitting the per-access lazy-init guard branch). Idiomatic stable-Rust form; behavior unchanged, all tests green. Not benchmark-claimed — the local measurement environment is too contended for reliable hot-path arbitration.
- Documented Mnemosyne's research foundations in the README: a table mapping each implemented technique to its source (mimalloc free-list sharding MSR-TR-2019-18, snmalloc message-passing ISMM 2019, jemalloc decay, hardened_malloc/Scudo guard pages) and an honest performance-positioning paragraph noting the single-threaded small-alloc gap to mimalloc is localized to the thread-local accessor and gated on a clean benchmark environment.
- Made branded heap containers allocation-free for zero-sized element types: `BrandedBox<T>` uses a dangling pointer and only runs `T`'s destructor when `size_of::<T>() == 0`, while `BrandedVec<T>` uses `usize::MAX` sentinel capacity, checked length growth, and no backing segment. Also changed `BrandedVec` non-ZST growth to use checked capacity doubling before layout construction.
- Extended the branded heap ZST contract to the primitive API: `BrandedHeap::alloc_init::<T>` now succeeds without allocation for zero-sized `T`, while `free` and `free_uninit` skip raw allocator deallocation for ZST branded blocks and still run destructors where required.
- Made `BrandedHeap::realloc` ZST-aware: zero-sized source blocks no longer flow through usable-size, byte-copy, or raw-free logic, so ZST-to-nonzero realloc allocates only the destination block and ZST-to-zero realloc consumes and drops the value without allocator traffic.
- Fixed `BrandedVec::new::<ZST>` capacity reporting: it now installs the same `usize::MAX` sentinel as `with_capacity`, preserving `len <= capacity` after allocation-free ZST pushes.
- Made `BrandedVec::into_boxed_slice` attempt an explicit allocate-copy-free shrink when `capacity > len`, so successful replacement no longer retains the original oversized allocation through the standard-policy same-pointer realloc fast path while allocation failure preserves the original buffer.
- Added a compile-time `AllocPolicy::RANDOMIZE_ALLOCATION` flag and wired secure/hardened policies through seeded page free-list permutations while leaving `StandardPolicy` on the existing lazy bump path.
- Routed `MnemosyneHeap` and `BrandedHeap` small-allocation paths through the canonical `ThreadAllocator::alloc_class` implementation, removing duplicated active-page pop/bump logic while preserving static policy dispatch.
- Restored the retained same-pointer shrink contract in `thread_realloc` by using `small_realloc_fits_existing_class` when `new_size <= layout.size()`.
