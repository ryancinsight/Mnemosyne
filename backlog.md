# Backlog

- [ ] [major] **WGPU-030, in progress; owner Codex; scope
  `mnemosyne-backend`, facade re-exports, backend selector impls/tests/docs, and
  release artifacts; last update 2026-07-13.** Remove the process-global WGPU
  raw-pointer staging backend. WGPU 30 exposes mutable mapped ranges only
  through a write-only view, so the `MemoryBackend` pointer contract cannot be
  implemented without violating the provider's memory model. Acceptance:
  obsolete callbacks and selectors are deleted, remaining backends pass the
  workspace gates, and Hephaestus owns explicit WGPU staging lifetimes.

- [x] [patch] Pin Eunomia and Melinoe once in the workspace SSOT for
  standalone-Git reproducibility.

## Atlas in-house replacement roadmap — mnemosyne slice [arch]

mnemosyne is the allocation SSOT. The GPU program (coeus/apollo using wgpu + cuda-oxide)
needs a first-class device-memory story beyond the current dlopen `CudaUnifiedBackend`:
- [x] [arch] Stage D1: device-memory strategy consumed by the `hephaestus` GPU substrate
  (atlas ADR 0001) — device buffer pools, page-locked/pinned host staging, explicit
  unified-vs-discrete policy through the `MemoryBackend` seam. Compose cuda-oxide
  allocation interop with the existing dlopen `cuMemAllocManaged` path; add wgpu
  buffer-pool hooks. ADR.
- [ ] [minor] Stage D1: melinoe-branded device buffers so ownership transfer between
  host/device/stream is a compile-time proof (pairs with hephaestus + coeus Stage D).

### Heterogeneous tiers + kernel resource budgets (atlas ADR 0002)
- [ ] [minor] Tier-keyed device pools: allocation keyed by themis
  `MemoryTier` (`Hbm` vs the new `Gddr`) + `PlacementHint`, with pinned-host
  (`HostPinned`) staging pools, behind the existing `MemoryBackend` seam.
- [x] [minor] `KernelResourceBudget` (`mnemosyne-core::kernel_budget`):
  registers/thread, shared-mem/block, threads/block with fully-`const`
  occupancy limiters (`blocks_limited_by_{registers,shared_mem,threads}`,
  `OccupancyLimits::blocks_per_unit` minimum). **Not** register allocation —
  GPU compilers assign registers (ADR 0002 constraint 2). Capacities arrive
  as plain quantities (themis `GpuTopology` accessor values) so
  mnemosyne-core stays `no_std`/dependency-free; unreported capacities
  surface as `u32::MAX` "no information", never a fabricated bound.
  Verification: closed-form Ampere-class fixtures, zero-budget/zero-capacity
  semantics, const-evaluability test. Remaining: shared-memory arena
  budgeting (the literal-allocation part) pairs with Stage D1 device pools.

## Closed

- [x] [patch] Stack-interner final-release critical section. The final entry
  and map-key `Arc` values are removed under the owning shard lock but dropped
  only after releasing it, preventing allocator/deallocation work from
  extending or re-entering the lock. Evidence tier: value-semantic and
  concurrent nextest coverage plus focused Criterion measurement.
- [x] [patch] `AlignedVec::into_vec` source-buffer release. Conversion keeps
  the required one-copy boundary into the standard `Vec` allocator and now
  drops the distinct aligned source allocation. Evidence tier: value-semantic
  nextest plus Miri nextest leak checking.
- [x] [patch] Page-metadata provenance and remote-free aliasing. Cached page
  addresses are refreshed through explicit exposed provenance before reuse;
  cross-thread frees mutate only the page-local atomic queue through raw-field
  access and never create an exclusive borrow of owner-managed metadata.
  Evidence tier: Miri under Stacked Borrows and Tree Borrows plus 125
  value-semantic nextest cases.
- [x] [patch] Refresh `mnemosyne-local` to Melinoe 0.9.0 so the allocator and
  scheduler graph resolves one validated executor-capability provider version.

- [x] [patch] Atlas provider graph refresh. `mnemosyne-local` now requires
  sibling Atlas `melinoe` `0.8.0`, and `Cargo.lock` resolves Themis to
  `0.9.17` so downstream Atlas consumers do not see a `melinoe ^0.7.0` versus
  `0.8.0` resolver conflict. Evidence tier: compile-time provider integration.
  Current gates: `rustup run nightly cargo check -p mnemosyne-local`;
  downstream `rustup run nightly cargo check -p kwavers-solver --lib`;
  downstream `rustup run nightly cargo clippy -p kwavers-solver --lib
  --no-deps -- -D warnings`.
- [x] [patch] Eunomia scratch local-source contract. `mnemosyne` and
  `mnemosyne-arena` resolve optional Eunomia support from the sibling Atlas
  checkout and removed the obsolete internal `num-complex` scratch feature after
  auditing the local Atlas consumer surface. Consumers should enable
  `mnemosyne/eunomia` and use `eunomia::Complex`; no remaining local
  `mnemosyne/num-complex` consumer was found. Evidence tier: compile-time
  validation plus value-semantic feature coverage. Current Atlas-checkout gates:
  `cargo check -p mnemosyne-arena --features eunomia`; `cargo nextest run -p
  mnemosyne-arena --features eunomia`; `cargo check -p mnemosyne --features
  eunomia`; `cargo nextest run -p mnemosyne --features eunomia`; package clippy,
  doctests, rustdoc, and no-default build checks for both packages.
- [x] [major] AR-2 WGPU callback registration soundness (superseded by ADR 0003). The public
  `WGPU_{ALLOCATE,DEALLOCATE}_CALLBACK` raw `AtomicPtr<c_void>` statics are now
  private `mnemosyne-backend` slots, and consumers register through the typed
  unsafe `register_wgpu_callbacks(WgpuAllocateCallback, WgpuDeallocateCallback)`
  API. The sibling `hephaestus-wgpu` staging callback registration was migrated
  in the same change set. Evidence tier: type-level function-pointer contract
  plus value-semantic Mnemosyne tests and downstream Hephaestus WGPU gates.
  Verification: focused Mnemosyne fmt/check/clippy/nextest/doctest/rustdoc
  gates and Hephaestus `hephaestus-wgpu` fmt/check/clippy/nextest (129/129).

## Closed

- [x] [major] WGPU callback registration publishes one immutable
  allocate/deallocate pair and rejects conflicting pairs. Concurrent readers
  observe only absent or one complete permanent pair. ADR:
  `docs/adr/0002-immutable-wgpu-callback-pair.md`. ADR 0003 subsequently
  removes this backend because WGPU 30 invalidates its pointer contract.

## Open

Filed from the 2026-07-13 allocator safety, memory, structure, and contention
audit, in priority order:

- [ ] [patch] Replace `mnemosyne-prof`'s global active-sample RMW and pointer
  modulo sharding only after Criterion profiles show the occupancy-mask and
  mixed-hash designs reduce contention without regressing allocator latency.
- [ ] [patch] Remove or compile out the dormant per-CPU cache's 720,896-byte
  static table while every production backend has `ENABLE_CPU_CACHE = false`;
  acceptance: binary-size evidence and unchanged allocator behavior.
- [ ] [arch] Split the 870-line profiler sampler by capture, slot lifecycle,
  and aggregation concern, and consolidate duplicated backend type lists at
  their deepest owning module without changing the hot-path representation.

Filed from the 2026-06-27 deep contention/memory audit (read-only fan-out over
arena/local/core/heap/backend). Ranked by value; each carries a testable
acceptance criterion and named blocker so it is Definition-of-Ready.

Added from the 2026-06-27 deep audit of the under-examined crates
(`mnemosyne-prof`, `mnemosyne-c-shim`, `mnemosyne-heap` containers):

- [ ] [patch] (Optional, low value) The cached-pointer fast path (check cell/OS
  slot; if non-null reconstitute + `is_allocating` guard + run; else init) is
  structurally repeated between `with_allocator` and `with_allocator_unguarded`
  within `CachedCellTls` and `NativeOsTls`/`AsmTls`. A shared helper
  parameterized over the slot accessor could factor it, but the providers
  genuinely differ in slot mechanism (OS key vs TEB asm vs `thread_local!` cell
  vs nightly static) and in guard-vs-unguard semantics, so the remaining overlap
  is small and a helper risks obscuring the hot path. Re-evaluate only if a
  fourth caching provider appears.

- [ ] [perf-experiment] Benchmark whether combining the lock-free pool bucket's
  `head` + `count` onto ONE cache line beats the current per-atomic isolation.
  Every push/pop touches both atomics, so a single 64-byte line would touch one
  line per op (not two) and halve the bucket BSS (64 B vs 128 B/bucket; ~96 KiB
  across 256 buckets x 6 backends). The current `TaggedSegmentStack` keeps them
  separate, matching the peer's deliberate "per-atomic cache-line isolation"
  choice — overturning it needs a clean Criterion A/B on the warm pool rows
  (huge cycle/dealloc, cross-thread handoff, segment cache eviction, burst
  retention), not just the threshold gate. Acceptance: A/B shows neutral-or-
  better on those rows -> combine + keep the BSS win; else keep separate +
  document the measured reason. Needs a quiet benchmarking machine (noise-
  sensitive, warm path). Rename `CacheAlignedAtomicPtr` (it is a tagged head,
  not a bare ptr) when this lands.

Filed from the 2026-07-01 four-agent audit cycle (high-severity findings were
fixed in the same cycle — see `## Completed`; these are the deferred
remainder, each Definition-of-Ready):

- [ ] [arch] AR-1: Mixed free-list-encryption policies over one backend are
  latently unsound at the chain level. **Decision recorded in
  [docs/adr/0001-free-list-encryption-mode-binding.md](docs/adr/0001-free-list-encryption-mode-binding.md)
  (Proposed — awaiting sign-off before implementation).** Owner-side paths
  (`pop_block::<P>`, `set_next::<P>`, `AtomicFreeList::push::<P>`) select
  encoding from the CALLER's static policy while the segment carries a dynamic
  `free_list_encrypted` flag; two policies with different
  `ENABLE_FREE_LIST_ENCRYPTION` on one backend share class pages, so one page's
  chain can mix encodings (reachable via the public `thread_alloc`/`thread_free`
  free functions, single-thread same-page — the 2026-07-01 orphan-adoption gate
  closed only the cross-thread instance). ADR recommends Option C (key the TLS
  allocator by encryption class: sound and zero-cost for the default policy)
  over dynamic-flag-everywhere (hot-path regression) or a `P==B` const-assert
  (conflates policy with backend). Blocker: ADR sign-off. **Step 1 (interim
  debug-assert safeguard at `Segment::cookie_for`) DONE 2026-07-02** — see
  `## Completed`; the remaining work is the type-level allocator-keying.
  Acceptance: an interleaved standard+hardened same-page chain-pop test
  round-trips without abort under a release build.
- [ ] [patch] AR-4: benchmark gate statistics are too weak for the 1.05
  threshold: `sample_size(10)` / 500 ms measurement yields CI widths the
  variance report itself flags at 15-25%. Fix: raise measurement time/samples
  for the gated rows (or gate on median with CI overlap), keep quick settings
  for exploratory rows. Blocker: quiet machine for re-baselining. Acceptance:
  gated-row CI half-width < the 5% threshold on the recorded baseline.
## Completed

- 2026-07-06 AR-8 [minor]: `mnemosyne-prof` stack interning now routes
  captured stacks across 64 cache-line-aligned shards by stack hash, encodes
  the shard in `StackId`, recycles ids per shard, and constructs first-seen
  `Arc<[usize]>` values outside the shard lock with a race-safe recheck before
  insertion. Added focused shard distribution, id-encoding, same-shard reuse,
  and concurrent distinct-shard interning tests. Added the real
  leak-detector-on Mnemosyne alloc/free Criterion group and summary filter.
  Current measured medians: small/32 `1.1940 us`, medium/1024 `1.1215 us`,
  large/8192 `1.1543 us` (10 samples, 500 ms measurement, Windows host).
  Evidence tier: value-semantic tests plus empirical Criterion measurement.

- 2026-07-02 consolidation cycle 3 (branch fix/audit-2026-07-soundness-perf,
  five atomic commits; detail in CHANGELOG.md and checklist.md). Closed:
  - **AR-1 step 1** [arch, interim]: ADR 0001's debug tripwire landed —
    `Segment::cookie_for::<P>` (the single encode/decode chokepoint) debug-
    asserts the policy's `ENABLE_FREE_LIST_ENCRYPTION` matches the segment's
    recorded mode; three latently-unsound integration tests restructured, a
    `should_panic` pin added, the contract documented on `thread_alloc`.
    The full type-level fix (allocator keyed by encryption class) remains
    open under AR-1 pending ADR sign-off.
  - **AR-7** [minor→major]: edition 2024 / resolver 3 across all 11 crates;
    `rust-version = 1.87` (clippy MSRV proved 1.85 dishonest —
    const `is_multiple_of`); 30 `unsafe extern`, 19 `#[unsafe(no_mangle)]`,
    granular `unsafe_op_in_unsafe_fn` blocks, style-2024 reformat. Breaking:
    consumers need Rust 1.87+.
  - **AR-9** [minor]: fuzz `c_shim_api` op-sequence mode (8-slot table,
    seeded write/verify oracles for adjacent-block clobber + realloc chains,
    bounded, leak-free on Drop; 9 smoke tests). libFuzzer run remains
    environment-blocked (g++ C++ runtime); `--lib` path is the evidence tier.
  - **AR-13** [patch]: one authoritative `mnemosyne-build-util` nightly probe;
    all THREE build scripts (prof, benchmarks, local) are thin callers. Also
    fixed a pre-existing latent `nightly_tls` E0432 in `mnemosyne-prof`
    (unconditional import of a `#[cfg(not(nightly_tls_active))]` item), masked
    on this host by the PATH-shadowed nightly rustc; verified by forcing
    `RUSTC` at the real nightly binary.

- 2026-07-02 consolidation cycle 2 (branch fix/audit-2026-07-soundness-perf,
  five atomic refactor commits; detail in CHANGELOG.md and checklist.md).
  Closed deferred items:
  - **AR-3** [patch]: cross-thread reclaim count moved to a per-`ThreadAllocator`
    field folded on `Drop`; the global `fetch_add` is off the reclaim hot path.
    First acceptance clause (no global RMW on the reclaim path) met and
    regression-tested for exact count; the "cross-thread benchmark rows
    neutral-or-better" clause folds into AR-4's re-baseline (a quiet machine),
    tracked there.
  - **AR-5** [patch]: benchmark bodies deduplicated to one generic
    `bench_iter_case`/`bench_batched_case<A>`, one `snmalloc_skips` predicate,
    one `GATE_ROWS` SSOT threshold table (row names unchanged; measured regions
    byte-identical). Follow-up: the nightly-rustc probe is still duplicated
    across `mnemosyne-prof/build.rs` and `mnemosyne-benchmarks/build.rs` — a
    shared build-util/xtask is the fix (filed as AR-13 below).
  - **AR-6** [patch]: local/core SSOT batch — shared `commit_in_place_free` +
    `do_local_free_internal` delegation, `is_sole_active_page`,
    `move_page_between_lists_branded`, `round_up_size` routing,
    `current_thread_id`, `detach_and_release_segment`, `get_next/set_next`
    forwarding, `abort_on_corruption` module, `parent_segment`/`cookie_for`/
    `locate_segment`, `recycle_sweeps` wired, `thread_realloc` branch flatten,
    `cfg(test)` `ThreadAllocator::alloc`, `PER_CPU_CACHE` single-implementor
    invariant documented, `core/types/page.rs` split into leaf modules,
    `kernel_budget` doc fix. `local/local_alloc/page.rs` left un-split
    (≤500 after consolidation, cohesive).
  - **AR-10** [patch]: FOLD `SecurePolicy`/`HardenedPolicy` into
    `mnemosyne-core::policy` (SSOT; core is dep-free); `mnemosyne-hardened`
    retained as a thin real re-export because external (gaia, kwavers) and
    internal manifests reference the crate name.
  - **AR-11** [minor]: `HasSegmentPool` → one required `pools()` with default
    accessors; six per-backend blocks collapse to `BackendPools::new()` +
    one-line impls (−77 lines); `MockBackend` fixtures migrated as the
    consumer half.
  - **AR-12** [patch]: `HandoffBuffer` `unsafe impl Sync` SAFETY comment added.

- 2026-07-01 audit cycle (branch fix/audit-2026-07-soundness-perf, eleven
  atomic commits; per-item detail in checklist.md and CHANGELOG.md):
  [patch] orphan-adoption key preservation + policy-compatibility gate with
  differentially-verified regression tests; [major] `BrandedCell` invariance
  + `unsafe BrandedBlock::cast` (both were safe-code UB); [patch]
  `TaggedSegmentStack::pop` Acquire failure ordering; [minor] huge-pool fit
  cap / derived bucket count / splice restore + huge-pool stats; [minor] CUDA
  module split with init-race atomics, probe-window VEH (silent
  ExitProcess(0) masking removed), full-scan unregister (device-allocation
  leak), test-runner detection deleted; [patch] prof hasher mixing, disabled-
  state sample drain, active-flag serialization, inverted leak flag; [patch]
  decay shutdown lost-wakeup handshake + dead DefaultBackend sweep; [patch]
  c-shim dump_leaks saturation; workspace profiles + committed nextest
  budget; no-op marker features and dead `SpinLock` removed.

- [patch] Repair `mnemosyne-arena` tagged-stack construction for Atlas
  consumers and make huge-pool rejected-chain restoration use the production
  `TaggedSegmentStack::push_chain` batch CAS path. `CacheAlignedAtomicPtr::new`
  is again the no-argument empty-head constructor, and `restore_rejected`
  computes tail/length once before pushing the private chain back as one batch.
  Verification: arena fmt/check/clippy plus downstream Kwavers FWI nextest
  59/59.

- [patch] Add `fuzz/c_shim_api` cargo-fuzz coverage for the
  `mnemosyne-c-shim` ABI. The target accepts arbitrary `(op, size, nmemb,
  alignment)` bytes, shapes them into resource-bounded hostile cases
  (zero-size, invalid alignment, over-`SEGMENT_SIZE` alignment, overflow, exact
  segment edge, and small writable requests), and routes every case through the
  real exported C ABI functions. Assertions pin null-or-valid allocation
  results, alignment, usable-size lower bounds, zeroed calloc prefixes, and
  initialized realloc preservation. The executor also builds as a normal
  no-libFuzzer library for local smoke tests. Local `cargo fuzz run` execution
  is blocked on this Windows install because GNU lacks sanitizer coverage
  support for the target and the MSVC SDK `kernel32.lib` is not installed.

- [patch] Document the `mnemosyne-c-shim` alignment ceiling. The `align <=
  SEGMENT_SIZE` (2 MiB) bound enforced upstream is now stated in the
  `aligned_alloc`/`posix_memalign` rustdoc and `include/mnemosyne.h`: an
  over-large `alignment` yields `NULL` (aligned_alloc) / `ENOMEM`
  (posix_memalign, with `*memptr` untouched), so callers can distinguish it from
  OOM. Doc-only; `cargo doc` clean. The behavior was already covered by the
  adversarial tests added previously; this closes the documentation half of that
  residual (the `cargo-fuzz` infra half remains filed). Also independently
  verified the peer's freshly-merged `shrink_to_len` + NUMA-steal SSOT
  (`numa_bucket.rs` `steal_from`) consolidations: full gate green (256 workspace
  tests, fmt, clippy `-D warnings`), both sound.

- [patch] Consolidate `BrandedVec` shrinking into one `shrink_to_len` helper.
  `shrink_to_fit` and `into_boxed_slice` now share the free-empty/realloc-to-len
  mechanics while `into_boxed_slice` keeps ownership-transfer-specific slice
  construction in place. The residual `extend_trusted` fast path remains
  unmeasured and unfiled until benchmark evidence shows the repeated capacity
  check matters.

- [patch] Consolidate wrap-around NUMA bucket stealing in
  `segment/pool/numa_bucket.rs`. `huge_pool.rs` and `segment_pool.rs` now share
  one `NUMA_BUCKETS` constant, one Themis-backed bucket-index conversion, and
  one generic `steal_from(start, pop_fn)` traversal, leaving each caller to own
  only its pool-specific pop operation. Direct tests pin wrap order and
  early-hit behavior; arena package gates are green.

- [patch] Consolidate the lock-free pool CAS loop into a `TaggedSegmentStack`
  SSOT (`segment/pool/tagged_stack.rs`) and harden it with direct tests. Both
  `NodeHugeBucket` and `NodeSegmentPool` open-coded the identical ABA-immune
  tagged-pointer push/pop/`take_all` CAS loops over `CacheAlignedAtomicPtr`;
  they now embed one `TaggedSegmentStack` (head + retained count) and layer only
  their own cap/telemetry on top, so the ordering + ABA-tag discipline lives in
  exactly one place (SSOT for the safety-critical contention-free path). Because
  the new struct holds only atomics, the FOUR hand-written `unsafe impl
  Send/Sync` (2 per pool) are deleted in favor of compiler-derived
  `Send`/`Sync` — a real reduction of the unsafe surface. Added 3 direct tests
  (LIFO + count, `take_all` chain/count, and a 4-thread×20k-iter conservation
  stress proving the ABA-tag loses no segment), complementing the existing
  pool-level conservation integration tests. Verification: fmt, clippy
  `-D warnings`, 38 arena tests, 254 workspace tests, arena doctests, `cargo doc`
  clean, and `benchmark_summary --enforce-thresholds` (all 12 gated rows within
  threshold). The change is codegen-neutral (the `#[inline]` methods inline into
  the same call sites), so it is a maintainability/safety/test consolidation —
  not a perf change; the head/count cache-line layout is unchanged (a combine is
  filed as a benchmark experiment above).

- [patch] Harden the `mnemosyne-c-shim` C ABI surface with adversarial
  hostile-input tests (the repo mandates panic-free, UB-free, no-unbounded-alloc
  handling of every FFI input). Added 10 tests pinning the boundary contracts the
  happy-path suite omitted: `aligned_alloc` zero/non-power-of-two/over-2-MiB-
  alignment all return null without UB; `aligned_alloc(align, 0)` is null-or-
  freeable; `realloc` shrink preserves `min(old_usable, new)` bytes;
  `posix_memalign` null-memptr/non-pow2 → `EINVAL` (memptr untouched), unsupportable
  alignment → `ENOMEM` (untouched, no UB); `malloc(usize::MAX/isize::MAX+1)` → null;
  `calloc` overflow pairs → null; and a deterministic `(size, alignment)`-grid
  sweep asserting every result is null-or-(aligned+writable+freeable). All pass —
  the boundary is verified sound (no bug found), and the suite is now a regression
  guard. Verification: fmt, clippy `-D warnings`, 23 c-shim tests, 251 workspace
  tests, `cargo doc` clean. Corrected a false prior audit claim in the process
  (`posix_memalign` ENOMEM-for-too-large-alignment is POSIX-correct, not a bug).

- [patch] Consolidate the `BrandedVec` grow mechanics into one `grow_to(new_cap)`
  SSOT (DRY). `push` and `reserve` each open-coded the identical
  `Layout::array → alloc-when-empty / realloc-otherwise → update ptr/cap`
  sequence (~15 lines x2); now both call the single `grow_to` helper and keep
  only their own capacity *policy* (push: initial-4 then ×2; reserve:
  `max(cap*2, needed)`). Correction to the filing: the earlier audit's claim of
  "divergent ×4 vs ×2 growth policies" was wrong — both already used `×2`; the
  `4` in `push` is the initial capacity, and `reserve` sizing to exact `needed`
  is correct, so there was no behavioral bug, only the mechanics duplication. The
  change is behavior-preserving, verified by the existing growth tests that pin
  `capacity()==4` after the first push and reserve sizing (`traits.rs`/`vec.rs`).
  Net subtractive (removed two now-dead imports in `ops.rs`). Verification: fmt,
  clippy `-D warnings`, 51 heap tests, 239 workspace tests, 8 heap doctests,
  `cargo doc` clean.

- [patch] Reduce `mnemosyne-prof` leak/dump memory pressure and contention.
  Live samples now store fixed-width `StackId` handles instead of per-allocation
  `Box<[usize]>` stacks; a refcounted `StackInterner` stores one `Arc<[usize]>`
  per distinct live call stack, increments the refcount on repeats, removes the
  entry on the last free, and recycles id slots. Stack capture uses a fixed
  stack buffer, so repeat call sites do not allocate a boxed frame array.
  `dump_profile` and `dump_leaks` clone active samples into an `ActiveSample`
  snapshot while holding each shard mutex, then release the lock before
  symbolication and file writes. The duplicated nightly/stable TLS sample-insert
  body now routes through `maybe_record_sample`, and pointer-to-shard routing is
  centralized in `sample_shard`. Verification: fmt, stable and nightly-TLS
  checks, clippy `-D warnings`, 7 prof nextest tests including
  `stack_interner_reuses_ids_and_releases_last_reference` and
  `active_sample_snapshot_is_detached_from_live_shards`, prof doctests, and
  `cargo doc`.

- [patch] Close the `// SAFETY:` discipline gap across the **`mnemosyne-prof`**
  crate (25 sites: `tls.rs` 14, `lib.rs` 10, `sampler.rs` 1). The fragile sites
  are now grounded: the TEB inline-`asm!` reads/writes state the Windows x86-64
  TEB layout they rely on (`gs` = TEB base; `gs:[0x1480 + i*8]` = `TlsSlots[64]`;
  `gs:[0x30]` = TEB self-pointer; `TEB+0x1780` = `TlsExpansionSlots`), with
  `# Safety` rustdoc on the two `unsafe fn get/set_teb_tls_slot`; the
  `core::mem::transmute(hook_ptr)` sites state the published-fn-pointer invariant
  (`register_*_hook` stored a real `unsafe extern "C" fn` under Release/Acquire);
  and every `&mut *get_profiler_state()` / `#[thread_local] static mut
  THREAD_STATE` access states the thread-local exclusivity + `in_hook`/`enter_hook`
  re-entrancy-guard invariant. The same sprint also fixed the latent
  `nightly_tls_active` `on_alloc` compile break by routing the allocation
  fast-path state check through `tls::should_skip_alloc_fast_path` instead of
  reaching into private TLS state from `lib.rs`. Verification: fmt, clippy `-D
  warnings`, prof nextest, prof doctests, `cargo doc`, and nightly
  `mnemosyne-prof --features nightly_tls` compile check. This completes the
  crate-by-crate SAFETY sweep across arena/local/core/heap/prof.

- [patch] Close the `// SAFETY:` discipline gap across the **`mnemosyne-heap`**
  crate — the crate the prior arena/local/core closures had missed. Every
  `unsafe` block in `raw_heap.rs` (45 sites), `heap.rs`, `brand.rs`,
  `branded_vec.rs`, `branded_vec/{ops,traits}.rs`, and `branded_box.rs` now
  carries a grounded `// SAFETY:` comment, and both bare `unsafe impl Send`
  (`heap.rs` `Heap`, `raw_heap.rs` `RawHeap`) state the brand-token
  thread-confinement invariant (`ThreadLocalToken<'brand>` is `!Send + !Sync`, so
  the heap cannot be *used* on another thread even if moved; the only interior
  state is `UnsafeCell<ThreadAllocator>` reached under that confinement). The
  GhostCell-style `BrandedCell::borrow`/`borrow_mut`/`borrow_mut_{2,3}` sites
  state the token-aliasing invariant; the raw `*_owned_unchecked` paths state the
  mask-recovered-segment and metadata-slot conventions. Comments only — 382
  insertions, 0 deletions, verified no non-comment line added. The audit also
  re-examined the suspected `insert` panic-safety and `extend` partial-state
  concerns and confirmed both sound (memory-safe contract warts, not bugs).
  Verification: fmt, clippy `-D warnings`, 239 workspace tests, 8 heap doctests,
  `cargo doc` clean.

- [patch] Remove the redundant `with_allocator_guard` TLS entry point (DRY/SSOT).
  It was an exact, zero-caller alias of `with_allocator` (which already arms the
  re-entrancy guard) propagated through two public traits (`TlsProvider`,
  `LocalAllocatorSelector`) — and implemented inconsistently: `native.rs`
  delegated to `with_allocator` while `stable.rs` carried full *duplicated* copies
  of the unsafe `&mut *(ptr as *mut ThreadAllocator)` cached-pointer reconstitution.
  Deleted the method from both trait definitions, the backend-selector macro arm,
  and all six provider impls, shrinking both the API surface and the unsafe-code
  surface to one guarded entry point. Live hot paths (`with_allocator`,
  `with_allocator_unguarded`) untouched, so hot-path codegen is byte-identical
  (the removed method had no callers and emitted no code). Verification: workspace
  builds with no broken caller (proving it was dead), fmt/clippy `-D warnings`,
  239 workspace tests, doctests, `cargo doc` clean.

- [arch] Close the ABA-immunity gap in the lock-free **segment** cache
  (`NodeSegmentPool`), the complement to the huge-pool tagged fix above. Its
  plain `AtomicPtr` head left single-element `pop` ABA-exposed (a stale
  `head X -> next Y` CAS after X is popped+re-pushed orphans the chain and loses
  segments). New `tests/segment_pool_concurrency.rs` provoked the loss; the
  pre-existing `test_concurrent_aba_safeness` missed it by never asserting
  conservation. Head now uses the tagged `CacheAlignedAtomicPtr` (48-bit addr +
  wrapping tag, mirroring `mnemosyne-core::sync::AtomicFreeList`). Stress test
  passes 15/15 (was non-deterministic loss); 239 workspace tests + threshold
  gate clean. Commit `241b795`.

- [patch] Add opt-in `mnemosyne-local/dealloc-probe` branch-mix counters for
  committed `thread_free` arms, with feature-gated value-semantic coverage that
  layout-proven same-owner small frees record as `InPlaceSmall`.
- [patch] Convert the huge-allocation cache's `NodeHugeBucket` from a
  spinlock-protected intrusive list to a lock-free Treiber stack. Exact-bucket
  pops still find a fitting segment behind undersized heads by restoring
  temporarily rejected segments, and shared cache-line atomic wrappers now live
  in `segment/pool/cache_aligned.rs`. The head carries a 64-bit tagged-pointer
  mutation counter to prevent stale-head ABA under concurrent pop/push stress.
- [patch] Resolve the `NodeHugeBucket` alignment tradeoff by replacing
  whole-struct `#[repr(align(64))]` with per-atomic cache-line isolation for the
  contended `head` and `count` fields.
- [patch] Expand the benchmark-summary threshold gate to the selected realloc
  latency rows and refresh `allocator_baseline_excerpt.csv` so enforcement now
  compares twelve selected rows.
- [patch] Clean backend/arena/tiered-heap rustdoc links and evidence wording so
  `cargo doc --workspace --all-features --no-deps` is warning-clean.
- [patch] Continue unsafe-discipline closure in `mnemosyne-core` and
  `mnemosyne-local` by documenting the `Segment` `Send`/`Sync`, Windows TEB
  thread-id read, local-free unchecked pointer/cookie updates, and native/ASM
  TLS allocator-pointer dereference invariants.
- [patch] Close the unsafe-discipline `// SAFETY:` gap across `mnemosyne-arena`
  (arena coordination, segment alloc/pools, and the scratch buffer module);
  every `unsafe` block and `unsafe impl Send/Sync` now documents its invariant,
  with two behavior-neutral consolidations (huge-pool purge drain loop, cached
  huge-segment header read) and the vacuous `ScratchPool::capacity` comment
  replaced by the real `!Sync` invariant.
- [patch] Consolidate initialized large/huge allocation fallback branches into
  one allocator helper in `mnemosyne-local::alloc`.
- [patch] Bound per-CPU cache CPU-id refresh retries after failed CAS attempts
  so each allocation/free attempt performs at most one refresh probe.
- [patch] Consolidate page-local free-list pop and lazy bump allocation into a
  single allocator helper used by `thread_alloc` and `ThreadAllocator`
  allocation paths.
- [patch] Route orphan-segment adoption through the guarded segment-aware
  page remote-free reclaim helper, avoiding empty-queue atomic drains while
  preserving adoption ownership and encryption semantics.
- [patch] Move allocation-side remote-free empty-queue guarding into
  `try_reclaim_and_allocate` so active and full page recovery share one
  helper-owned drain path.
- [patch] Centralize allocator sweep remote-free reclamation through a
  segment-aware `Page` helper that skips empty queues before atomic drains.
- [patch] Skip empty page-local remote-free queues during thread-exit
  owned-segment reclamation, avoiding unnecessary atomic drains while
  preserving live-segment orphaning semantics.
- [patch] Skip empty page-local remote-free queues during periodic allocator
  defragmentation sweeps, avoiding unnecessary atomic drains while preserving
  live-allocation accounting.
- [patch] Route allocator segment reclamation sweeps through a segment-aware
  page cross-thread-free reclaim helper, avoiding repeated parent segment and
  page-index derivation where the caller already owns that metadata.
- [patch] Remove benchmark-summary CSV row `Vec<Cow<_>>` collection and
  benchmark-name clone by parsing required summary fields through a lending
  `Cow` iterator.
- [patch] Remove the missing-selected-benchmark `Vec` allocation from
  `benchmark_summary` threshold enforcement.
- [patch] Remove the benchmark-baseline comparison `Vec` allocation and
  benchmark-name clone from `benchmark_summary` by streaming borrowed
  comparison rows.
- [patch] Remove the selected-baseline excerpt `Vec` allocation from
  `benchmark_summary` by streaming selected rows through an iterator writer.
- [patch] Split `mnemosyne-prof` TLS provider and per-thread hook state into a
  dedicated leaf module, leaving public controls and hook entry points in the
  crate root.
- [patch] Split `mnemosyne` global allocator integration tests into
  bounded-context leaf modules while keeping the root as global allocator and
  shared fixture ownership only.
- [patch] Replace duplicate local allocator TLS seed cache branches with the
  Melinoe thread-cached initialization primitive.
- [patch] Split `mnemosyne-heap` unit tests into bounded-context leaf modules
  under `src/tests/`, keeping the root module as shared fixtures only.
- [patch] Remove benchmark-summary CLI argument collection by parsing known
  flags directly from the iterator with value-semantic parser coverage.
- [patch] Refresh `benchmarks/allocator_comparison.md` with current
  `system-jemalloc` Criterion results and close the initial segment-cache
  eviction alert as measured variance after focused rerun plus threshold gate.
- [patch] Split the benchmark-summary binary into dedicated config, CSV,
  Criterion, report, allocator-rendering, metadata, and threshold leaf modules;
  remove tracked scratch artifacts; and harden report writers so missing
  `target/criterion` parents are created before output files are opened.
- [minor] Add `ScratchBank<T, const N>` as the provider-owned fixed scratch-role abstraction for Apollo transform workspaces, keeping role selection const-generic and avoiding repeated per-role `ScratchPool` statics in consumers.
- [patch] Prevent combined usable-size benchmark cross-optimization by consuming the allocated pointer through `black_box` before size query and deallocation, resolving the stale inverted small/medium/large ordering in `usable size latency`.
- [patch] Add layout-proven `GlobalAlloc::dealloc` routing so Rust callers with the original `Layout` monomorphize out the large/huge free classifier for small allocations while preserving the pointer-only `thread_free` classifier for C-style and unknown-layout callers.
- [patch] Outline active-profiler free-size accounting behind a cold helper so disabled profiling leaves the hot free path with only the existing activity guard.
- [patch] Add active `rpmalloc::RpMalloc` benchmark coverage and reduce the `large_8192` deallocation row by stamping owner allocator cache pointers, bypassing the busy-bit write pair for first frees from full pages, and moving full pages back to active pages with one branded list token.
- [patch] Remove duplicate public cold-allocation defrag cadence charging after `ThreadAllocator::alloc_cold`; the cold refill now charges once at the owning allocator boundary.
- [patch] Add GhostCell-style branded page-list mutation tokens for intrusive active/full/empty page lists, keeping page-list splice and push helpers zero-sized and allocator-permission-gated.
- [patch] Add GhostCell-style branded owned-segment mutation tokens for the intrusive owned-segments list and a Miri-only owner-token fallback that avoids unsupported Windows inline assembly.
- [patch] Carry one branded page-list token through empty-page recycling selection and unlink, preserving dirty-segment prioritization while reducing repeated token/unlink setup on `pop_best_empty_page`.
- [patch] Reject page-local pop/bump helper consolidation in `alloc_class`/cold active-head retry because the monomorphized helper perturbed allocation-cycle codegen and exceeded selected cycle thresholds.
- [patch] Reject current-segment minimum-block free shortcut because focused deallocation-only rows regressed despite improving one noisy small-cycle sample.
- [patch] Reject skipping `initialize_free_list` for never-used fresh pages because the refill-row improvement regressed all selected allocation-cycle gates.
- [patch] Reject direct full-page relink and layout-aware small-free experiments after measurement or invariant checks failed to support retaining them.
- [patch] Restore first-class RpMalloc columns in `allocator_comparison.md` generation so rpmalloc benchmark rows are visible in the comparator table.
- [patch] Reject active-page empty-`thread_free` guards after threshold enforcement showed cycle-latency regressions; keep the existing unconditional active-page reclaim path.
- [patch] Make `nightly_tls` compiler-channel-aware so stable all-feature gates use the portable TLS provider and nightly compilers retain the `#[thread_local]` fast path.
- [patch] Make `nightly_tls_active` build-script cfg generation rerun when `RUSTC` changes, preventing stale compiler-channel detection.
- [patch] Maintain an allocator-local owned-segment count so segment reclaim and defragmentation threshold checks no longer rescan the owned list.
- [patch] Split thread-local segment ownership and reclamation into `local_alloc/segment/ownership.rs` and `local_alloc/segment/reclaim.rs`.
- [patch] Remove per-iteration heap allocation from the cross-thread handoff benchmark, run the jemalloc-enabled allocator comparison, and refresh the threshold-gated benchmark baseline after verifying stale cross-thread and saturated-threaded rows against unmodified `HEAD`.
- [patch] Remove non-owner defragmentation accounting from remote-free enqueue, resolving the `cross-thread free handoff/mnemosyne/small_32` regression while preserving owner-side reclamation.
- [patch] Replace threaded allocation-cycle worker `Vec` storage with fixed arrays and regenerate the jemalloc-enabled comparison, resolving the stale `threaded small allocation cycles/mnemosyne` regression row.
- [patch] Align small-allocation `usable_size` page-index derivation with the deallocation classifier and regenerate stable usable-size comparison rows.
- [patch] Move thread-local allocator statistics into a dedicated leaf module and compute diagnostic snapshots from active/full/empty page lists instead of segment-wide page scans.
- [arch] Consolidate the two public heap wrapper surfaces into one scoped `Heap<'brand, P, B>` API backed by the single monomorphized `RawHeap<P, B>` implementation.
- [patch] Supersede the earlier wrapper-column allocator report shape: `MnemosyneHeap` and `BrandedHeap` are no longer classified as allocator comparators, and stale Criterion rows are ignored.
- [patch] Include SnMalloc `huge_2m` benchmark rows in allocator comparisons instead of hard-coded `N/A` omissions.
- [patch] Replace remaining Rustdoc example unwraps with contextual allocation diagnostics.
- [patch] Replace benchmark utility unwraps with explicit full-page handling and contextual layout diagnostics.
- [patch] Replace the local allocator page-saturation test panic with a value assertion carrying segment/page diagnostics.
- [patch] Replace remaining branded vector shrink and extension unwraps with operation-specific diagnostics.
- [patch] Replace bare branded container trait-operation unwraps with operation-specific diagnostics.
- [patch] Replace bare branded-cell test allocation and vector-push unwraps with operation-specific diagnostics.
- [patch] Replace bare branded-vector transition push unwraps with operation-specific diagnostics.
- [patch] Replace bare heap unit-test layout unwraps with a shared contextual layout helper.
- [patch] Replace bare heap integration test layout and worker-join unwraps with contextual diagnostics.
- [patch] Harden global allocator leak-detector integration test with guarded profiler/allocation cleanup and contextual dump diagnostics.
- [patch] Replace bare policy integration test layout/thread-join unwraps with contextual diagnostics.
- [patch] Harden local topology tests with contextual diagnostics and an RAII guard for the global per-CPU cache flag.
- [patch] Replace the remaining bare C-shim leak-report `CString` unwrap with contextual diagnostics.
- [patch] Reconcile `complexity_audit.md` with the current free-list/bump-page allocator after the bitmap free-list experiment was rejected.
- [patch] Replace bare segment-layout unwraps in `mnemosyne-core::types` tests with a single contextual layout helper.
- [patch] Harden `mnemosyne-prof` integration tests with contextual diagnostics and RAII cleanup for global profiler state and live thread allocations.
- [patch] Remove production panic paths from OS TLS key initialization; native TLS failure now falls back to standard thread-local state for allocator and profiler access.
- [patch] Relax profiler OS TLS-key publication to relaxed atomic ordering; the key is an immutable slot index and protects no Rust memory dependency.
- [patch] Harden profiler sample storage against poisoned shard locks and bounded stack-capture overflow while preserving exact retained stack slices.
- [patch] Clean up the clippy-reported nested occupancy-mask branch in `Page::set_alloc_count_for_segment`.
- [patch] Include the `Threaded medium allocation cycles` Criterion group in benchmark-summary extraction and generated allocator comparison reports.
- [patch] Pin benchmark-summary active-group filtering with unit tests so all allocator benchmark groups are retained and exploratory TLS benchmark rows stay out of allocator comparison summaries.
- [patch] Make benchmark threshold enforcement reject incomplete current Criterion data when any selected baseline row is absent.
- [patch] Document the generated variance report and selected-row completeness requirement in the benchmark workflow.
- [patch] Apply the scheduler-aware variance threshold to retained medium-threaded allocation rows.
- [patch] Report `Mnemosyne`, `MnemosyneHeap`, and `BrandedHeap` as distinct allocator comparison rows using exact allocator classification.
- [patch] Convert the remaining `BrandedHeap` benchmark allocation failure from `expect` panic to explicit benchmark failure diagnostics.
- [patch] Consolidate explicit and branded heap mechanics behind a shared monomorphized `RawHeap<P, B>`.
- [patch] Keep `MnemosyneHeap` available from `mnemosyne_heap` while removing it from the top-level `mnemosyne` shell re-export.
- [patch] Keep `RawHeap` large/huge deallocation code in one cold helper shared by explicit and branded free paths.
- [patch] Preserve profiler sample memory efficiency with exact captured stack slices while retaining sharded active-sample maps.
- [patch] Retain the active thread-local segment during local frees so hot allocate/free cycles reuse page free lists instead of scanning and recycling the segment.
- [patch] Replace single-shape allocator benchmarks with Criterion cycle, burst-retention, and threaded comparison groups for Mnemosyne, mimalloc, and snmalloc.
- [patch] Fix Unix backend constant typing so Rustfmt can parse all target modules.
- [patch] Add Mnemosyne backend and arena memory telemetry for mapped bytes, peak mapped bytes, map/unmap calls, retained free segments, and retained free bytes.
- [patch] Bound the global free segment cache to one segment-turnover window and release additional empty segment mappings to the OS.
- [patch] Add cross-thread free handoff benchmarks for Mnemosyne, mimalloc, and snmalloc.
- [patch] Avoid invoking segment-reclaim logic on hot local frees when the page belongs to the current thread-local segment.
- [patch] Add current-thread live allocation, current-thread owned segment, and cross-thread reclaimed block telemetry.
- [patch] Add `memory_report` CSV output for direct Mnemosyne memory telemetry inspection.
- [patch] Replace per-iteration cross-thread benchmark thread creation with persistent bounded-channel handoff workers.
- [patch] Add per-size-class occupancy telemetry for active pages, empty pages, live allocations, and total slots.
- [patch] Replace threaded allocation benchmark thread creation with persistent bounded-channel worker sets.
- [patch] Add deterministic segment-cache eviction benchmark coverage and `memory_report` eviction telemetry.
- [patch] Add arena purge telemetry for purged segments, purge calls, and purged bytes.
- [patch] Add `benchmark_summary` release command that extracts compact Criterion mean/median estimates to CSV.
- [patch] Add `purge_after` memory report scenario proving retained segment cache purge behavior.
- [patch] Add source-controlled selected Mnemosyne benchmark baseline excerpt.
- [patch] Add benchmark baseline metadata documenting platform, toolchain, and benchmark commands.
- [patch] Add current-to-baseline benchmark comparison CSV generation for selected Mnemosyne rows.
- [patch] Restore small-allocation segment pointer scope in `thread_free` after the large-allocation classifier.
- [patch] Add value-semantic tests for benchmark summary CSV parsing and baseline ratio computation.
- [patch] Restore missing assertion delimiter in the memory retention-bound test.
- [patch] Make the page-recycling test assert segment reuse and target size-class metadata instead of global-state-sensitive exact page index.
- [patch] Require explicit `--refresh-baseline` for source-controlled benchmark baseline mutation.
- [patch] Route cross-thread small frees to the owning page queue instead of the owner allocator queue.
- [patch] Remove duplicate segment-address derivation from `thread_free`.
- [patch] Preserve hot local allocation path by reclaiming page-local remote frees only after local free blocks are exhausted.
- [patch] Centralize page-local cross-thread free reclamation in an inlined `Page::reclaim_thread_free` method.
- [patch] Add direct value-semantic coverage for `Page::reclaim_thread_free`.
- [patch] Bind the global allocator and local allocator tests to the zero-sized `StandardPolicy` after policy-generic allocation APIs were introduced.
- [patch] Remove the panic-bearing `align_up` API and keep checked alignment as the single production alignment contract.
- [patch] Make benchmark regression threshold enforcement explicit with `--enforce-thresholds` so quick-mode summaries remain non-gating.
- [patch] Move generated benchmark metadata from `benchmarks/metadata.json` to `target/criterion/benchmark_metadata.json`.
- [patch] Stabilize page-recycling test allocation-count expectations against reusable orphan/global segment state.
- [patch] Gate benchmark metadata path constant out of test builds to keep diagnostics warning-clean.
- [patch] Centralize allocation initialization and free poisoning behind monomorphized `AllocPolicy` helpers.
- [patch] Serialize allocator integration tests that mutate process-wide segment-pool state.
- [patch] Derive hard regression threshold policy from repeated non-quick benchmark samples on the same hardware.
- [patch] Re-benchmark cross-thread 32-byte handoff against mimalloc after page-queue routing.
- [patch] Audit remaining allocator panic sites in tests and benchmark-only utilities.
- [patch] Convert benchmark-only panic assertions in memory_report to explicit Result errors.
- [patch] Replace raw segment owner pointers with a transparent `SegmentOwner` permission token.
- [patch] Remove allocator-level `incoming_free_list` after page-local remote-free routing made it redundant.
- [patch] Add direct test coverage for re-entrant local free fallback through the page-local atomic queue.
- [patch] Complete backend-specific segment-pool typing through `HasSegmentPool` exports and arena call-site bounds.
- [patch] Reject single-TLS local-free rewrite after focused benchmark showed a statistically significant regression.
- [patch] Reject `UnsafeCell` allocator permission split after focused cycle benchmark confirmed hot-path regression.
- [patch] Add a saturated threaded small-allocation benchmark group to isolate allocator throughput from bounded-channel worker coordination overhead.
- [patch] Fix backend-specific thread-local allocator selector generation so each backend receives distinct TLS storage.
- [patch] Run the saturated threaded small-allocation benchmark against Mnemosyne, mimalloc, and snmalloc.
- [patch] Add per-thread page-refill telemetry and defer recycle sweeps until the current segment is exhausted.
- [patch] Reject single-TLS local-free collapse after historical threaded benchmark exceeded the configured threshold.
- [patch] Replace the scheduler-sensitive historical threaded baseline gate with the saturated threaded baseline row.
- [patch] Convert benchmark runner panic assertions and channel unwraps to explicit benchmark failure diagnostics.
- [patch] Add local safety contracts to benchmark unsafe operations and allocator policy byte-initialization helpers.
- [patch] Audit backend-specific CUDA unified-memory tracking for bounded metadata and zero-cost fallback behavior.
- [patch] Synchronize README architecture notes with page-local remote-free routing and CUDA fallback behavior.
- [patch] Audit production unsafe blocks in `mnemosyne-backend` for local safety contracts and ordering minimality.
- [patch] Audit backend allocation failure accounting so telemetry cannot record unmapped bytes before OS release succeeds.
- [patch] Audit arena purge accounting so purged segment counters only count confirmed backend releases.
- [patch] Audit ignored backend release results in large-allocation cleanup paths.
- [patch] Audit large-allocation metadata layout for alignment guarantees and metadata-slot bounds.
- [patch] Audit small-allocation free classification for invalid-alignment and metadata-boundary failure modes.
- [patch] Audit allocator alignment request handling so invalid public `Layout` alignments cannot reach arena alignment math.
- [patch] Audit zero-size allocation behavior for `GlobalAlloc` and direct `thread_alloc` callers.
- [patch] Audit allocation request size bounds against `Layout` maximum and backend mapping arithmetic.
- [patch] Audit duplicated allocation request validation across global, local, and arena entry points.
- [patch] Tighten huge-allocation backend mapping size and pin the memory-efficiency contract with telemetry.
- [patch] Remove dead page back-pointer metadata and keep `Page` within one cache line.
- [patch] Audit generated benchmark artifact freshness and documentation references for the current allocator comparison set.
- [patch] Audit test-only panic diagnostics without reducing assertion strength.
- [patch] Audit production debug assertions for value-semantic invariant messages and zero-cost release behavior.
- [patch] Audit local allocator remote-free reclaim paths for duplicated block-pop logic.
- [patch] Investigate full all-allocator Criterion quick-run timeout while focused gated rows complete.
- [patch] Guard local-free full-page reactivation on confirmed full-list unlink.
- [patch] Audit benchmark baseline metadata after bounded Criterion harness configuration.
- [patch] Refresh source-controlled benchmark baseline excerpt from bounded Criterion harness output.
- [patch] Optimize thread_free segment owner check by introducing get_allocator_ptr to LocalAllocatorSelector.
- [patch] Add jemalloc to allocator benchmark comparator coverage and generated comparison reports.
- [patch] Add opt-in segment tail guards without default benchmark overhead.
- [patch] Extend memory report with page-reset and guard-install telemetry.
- [patch] Force cross-crate inlining for size-class mapping on allocator hot paths.
- [patch] Move secure-policy small-free poisoning after classification so the small page metadata lookup is shared.
- [patch] Reject layout-aware `GlobalAlloc::dealloc` small-free classification after saturated threaded benchmark regression.
- [minor] Add usable-size latency benchmarks for Mnemosyne, mimalloc, snmalloc, and target-gated jemalloc.
- [patch] Optimize `usable_size` small-allocation classification by reading target page metadata before the Page 0 huge-allocation fallback.
- [minor] Override `GlobalAlloc::realloc` with an in-place standard-policy fast path when the new request fits in `usable_size(ptr)`.
- [patch] Preserve secure-policy realloc zero-initialization by forcing replacement allocation on growth.
- [minor] Add realloc latency benchmarks for within-class and cross-class realloc cycles across Mnemosyne, mimalloc, snmalloc, and target-gated jemalloc.
- [minor] Add isolated usable-size query latency benchmarks that separate metadata lookup cost from allocation/deallocation cost.
- [minor] Add allocation-only latency benchmarks with drop-guard cleanup to separate allocation cost from deallocation cost.
- [minor] Add system allocator comparator rows to the allocator benchmark matrix and generated comparison reports.
- [patch] Optimize small-free classification and local-free owner checks to remove duplicate metadata and TLS work from deallocation hot paths.
- [minor] Add deallocation-only latency benchmarks to isolate free-side cost across Mnemosyne, System, mimalloc, snmalloc, and target-gated jemalloc.
- [patch] Remove dead `Page::local_free` state and allocation fast-path branch after verifying all local frees route through `Page::free`.
- [patch] Add small-realloc size-class proof fast path to avoid `usable_size` metadata lookup when the old `Layout` already proves the existing class covers the new request.
- [patch] Add a current-segment marker so same-thread frees on the active segment bypass the allocator-cell mutable borrow when no page-list mutation or segment reclaim is required.
- [minor] Add `LocalAllocatorSelector::with_allocator_guard` so allocation guard setup, allocator access, and guard clear happen inside one selector operation.
- [patch] Replace hot-path size-class arithmetic with a compile-time lookup table generated by `const` evaluation.
- [minor] Replace thread-local allocator `RefCell` access with guarded `UnsafeCell` access under the allocation flag.
- [patch] Add variance-aware benchmark report generation for Criterion mean confidence intervals and unstable-row classification.
- [patch] Centralize huge-allocation suffix sizing in `Segment::huge_mapping_suffix_from` and route `usable_size` plus secure free poisoning through it.
- [patch] Reject precomputed-class allocation dispatch and direct realloc-capacity arithmetic after focused Criterion rows showed threaded and realloc regressions.
- [patch] Reject layout-aware small-deallocation bypass after saturated threaded rows regressed despite isolated deallocation improvement.
- [patch] Document realloc slow-path copy bounds so size-class slack bytes are not propagated as initialized data.
- [patch] Collapse the per-thread allocation guard and allocator cache into one TLS slot, reducing small allocation/free cycle TLS lookups while preserving the re-entrant fallback contract.
- [patch] Reject forced cross-crate inlining of `AtomicFreeList` operations after cross-thread handoff improved but saturated threaded cycles regressed.
- [patch] Reject `thread_local!` const initialization for the allocator slot after it improved non-saturated rows but regressed saturated threaded cycles.
- [patch] Add all-size-class lower-bound coverage for `usable_size` so small allocations can never under-report class capacity.
- [patch] Reject separate owner-token TLS routing after cycle latency and cross-thread handoff regressed.
- [patch] Extract shared monomorphized realloc slow path so both allocator implementations use one copy-length contract.
- [patch] Force inlining of the shared realloc slow-path helper after focused Criterion rows improved both retained realloc latency regressions.
- [patch] Reject the <=128-byte arithmetic realloc capacity shortcut after its absolute point estimate missed the accepted within-class realloc row and polluted allocator-cycle measurements.
- [patch] Reject deferred remote-free telemetry accounting after it failed to improve small cross-thread handoff and regressed medium handoff plus historical threaded allocation cycles.
- [patch] Reject forced inlining of `Page::reclaim_thread_free` after refreshed historical threaded allocation cycles regressed despite one saturated sample improving.
- [patch] Reject forced inlining of exported `usable_size` after combined usable-size and allocator-cycle rows regressed.
- [patch] Reject a Layout-proven small-allocation entry split after it improved allocation-only latency but widened the retained small cycle and threaded-small gaps.
- [patch] Serialize backend telemetry tests that mutate process-wide mapping counters so workspace tests are deterministic.
- [patch] Reject compact `Page` counter layouts after 48-byte metadata experiments regressed saturated threaded and usable-size rows.
- [patch] Centralize the 16-byte small-block floor as `MIN_BLOCK_SIZE` and remove stale compact-counter invariants.
- [patch] Reject removing the `MAX_ALLOC_SIZE` check from the Layout-validated allocation predicate after focused Criterion rows improved cycle/usable means but regressed allocation-only and historical threaded small rows.
- [patch] Reject Bitmap Free Lists for classes 0, 1, and 2 after Criterion small allocation cycles, realloc, and threaded allocation benchmarks regressed.
- [patch] Reject Bounded Retention of Huge Mappings and per-CPU cache optimizations after allocator burst retention and threaded cycles regressed.
- [patch] Make branded heap containers allocation-free for zero-sized types and reject overflowing `BrandedVec` capacity growth before layout construction.
- [patch] Make primitive branded heap initialization/free ZST-aware so `alloc_init::<T>`, `free`, and `free_uninit` share the same allocation-free zero-sized-type contract as the safe containers.
- [patch] Make primitive branded heap realloc ZST-aware so zero-sized source permissions never route dangling pointers through usable-size, byte-copy, or raw-free allocator logic.
- [patch] Preserve the `len <= capacity` vector invariant for `BrandedVec::new::<ZST>` by installing the allocation-free sentinel capacity at construction.
- [patch] Make `BrandedVec::into_boxed_slice` attempt an explicit shrink instead of relying on same-pointer shrink realloc, while preserving the original buffer if replacement allocation fails.
- [patch] Wire secure and hardened allocation policies to seeded page free-list randomization while preserving the standard policy lazy bump path.
- [patch] Route heap-local small allocation through `ThreadAllocator::alloc_class` as the single active-page pop/bump implementation.
- [patch] Restore same-pointer shrink behavior in `thread_realloc` through the existing small-realloc size-class proof.
- [patch] Avoid allocate-copy-free churn for standard-policy large/huge half-shrink reallocs and bound all replacement realloc copies to `min(layout.size(), new_size)`.
- [patch] Reduce leak/profiling stack-sample memory by capturing into fixed stack storage and retaining only exact-length boxed stack slices.
- [arch] Split `mnemosyne-core` allocator types, `mnemosyne-arena` segment pools/tests, `mnemosyne-local` top-level allocation/free/realloc/TLS/options helpers, `mnemosyne-prof` sampling/reporting, `mnemosyne-c-shim` tests, and `BrandedVec` operations/trait impls into cohesive leaf modules while preserving public re-exports and monomorphized APIs.
- [patch] Stabilize memory-stat tests after leak-detector thread-exit orphan adoption by asserting allocation-count deltas instead of a false absolute baseline.
- [arch] Split heap, branded-container, local-allocation page/routing/segment, and global allocator test surfaces into cohesive modules while preserving monomorphized hot-path APIs and public re-exports.
- [patch] Remove stale imports from split local allocator modules so warning output stays clean and real allocator regressions remain visible.
- [patch] Retain `threaded medium allocation cycles/` in generated benchmark summaries and comparison reports.
- [patch] Use `benchmark_variance.csv` to retest remaining within-class realloc and historical threaded-row optimizations before accepting allocator changes.
- [patch] Investigate cross-thread handoff batching or owner-token routing without increasing saturated threaded cycles.
- [patch] Investigate mimalloc's remaining within-class realloc, historical threaded-row, saturated threaded-row, cross-thread handoff, and usable-size combined-cycle advantages after the unified TLS slot narrowed saturated threaded disparity.
- [patch] Run the jemalloc comparator leg on a target where `tikv-jemallocator` links and refresh comparison rows.
- [patch] Fix decay engine thread-spawning shadowing bug and add `decay_purger_reaches_steady_state` integration test.
- [patch] Expose `get_options` and `configure` in the top-level `mnemosyne` crate and verify via programmatic configuration tests.
- [patch] Add `multi_heap_isolates_allocation_streams` and `multi_heap_release_does_not_touch_other_heaps` integration tests.
- [patch] Consolidate public allocator periodic-defragmentation accounting into a shared `ThreadAllocator::record_defrag_operation` cold-sweep boundary.
- [patch] Reject extending the shared defrag-accounting helper to `RawHeap` after explicit/branded cycle rows regressed; heap-local hot paths retain their inline accounting shape.
- [patch] Split page allocation-counter updates into monomorphized increment/decrement helpers and pass known page indices through free paths so occupancy-mask maintenance avoids redundant page-index recovery.
- [patch] Route same-owner small cross-class realloc through the raw allocator pointer with an explicit re-entrancy flag, avoiding the closure guard overhead while preserving local free semantics.
- [patch] Bound periodic defragmentation owned-segment counting by the reclaim threshold instead of traversing the whole owned list once four segments are known.
- [patch] Iterate segment reclaim and defragmentation over the occupied-page bitmask instead of scanning every page in mostly empty segments.
- [patch] Relax hot TLS-key reads from acquire to relaxed ordering because the key is an immutable OS slot index, not a protected allocator data dependency.
- [patch] Store each page's segment-local index in metadata and route `page_start` plus occupancy-mask transitions through that stored index, avoiding repeated page-address subtraction/division while keeping `Page` within one cache line.
- [patch] Use page allocation-counter increment helpers on local and heap allocation hot paths so occupancy-mask maintenance does not reload and compare an already-derived target count.
- [patch] Refresh allocator comparison rows after stored page-index routing; current saturated threaded small cycles measure Mnemosyne `66.851 us` versus mimalloc `70.088 us`.
- [patch] Charge periodic defragmentation cadence only when local free transitions actually make a page empty, removing sweep accounting from full-page-to-active transitions and closing `allocator deallocation latency/large_8192` versus jemalloc.
- [patch] Keep current-segment occupancy-mask bits conservative across local frees, removing repeated mask clear/set traffic from hot small alloc/free reuse while preserving exact `alloc_count` authority.
- [patch] Derive `usable_size` page indices from the already-computed segment base, removing the shifted-mask index path and refreshing the small usable-size comparator row.
- [patch] Reject the `MAX_SMALL_ALLOC_SIZE` size-class boundary shortcut after the benchmark-summary threshold gate still reported `allocator cycle latency/small_32` above the retained 1.05 ratio despite large-cycle improvement.
- [patch] Replace runtime size-class leading-zero arithmetic with a compile-time-generated `u8` table covering every small allocation size, reducing allocator cycle latency without adding type-specific APIs.
- [patch] Update `melinoe` to the latest `main` commit resolved by Cargo (`85d498bb`, crate version `0.5.0`) and verify `mnemosyne-heap` against the current branded-token API.
- [patch] Remove per-row `Vec`, owned key, formatted-cell, and allocator-name lowercase allocations from `benchmark_summary` allocator comparison generation by splitting benchmark names with borrowed `&str` slices, keeping comparison keys borrowed, streaming cells through `Display`, and classifying allocators case-insensitively without allocation.
- [patch] Remove profiler dump snapshot clones and intermediate symbol vectors by processing active sample maps under shard locks, borrowing exact boxed stack slices, streaming leak samples directly to the report file, and using scoped `Path::to_string_lossy` `Cow` values only at the output boundary.
- [minor] Make the top-level `mnemosyne` branded heap re-export an additive default feature and build allocator benchmarks with `default-features = false`, keeping the default public API unchanged while isolating global allocator latency runs from branded-heap dependency code layout.

## Open

- [patch] status=done owner=codex scope=`crates/mnemosyne-local`,
  allocator regression tests, and PM artifacts; root-cause and eliminate the
  Miri-confirmed alloc/free page-metadata aliasing violation recorded in
  `gap_audit.md`. Acceptance: the Hermes reproducer passes under both Stacked
  Borrows and Tree Borrows, focused Mnemosyne value-semantic tests pass under
  nextest, and the fix introduces no allocator-cycle threshold regression.
- [patch] status=done owner=codex scope=`D:/atlas/worktrees/mnemosyne-ritk`
  on branch `codex/mnemosyne-0.2-page-provenance`; port the Miri-verified
  page-provenance correction onto the exact 0.2 provider line consumed by RITK.
  Acceptance: focused allocator nextest and Clippy pass, RITK pins the verified
  revision, and its registration wheel completes without a native crash.
  Rejected after audit: RITK already pins `477f957`, whose parent is the exact
  Miri-verified correction `5a9f49f`; no consumer pin change is required.
- [patch] Investigate the remaining `allocator deallocation latency/large_8192` gap to RpMalloc. Current retained comparison is Mnemosyne `40.909 ns` versus RpMalloc `6.871 ns` (`5.95x`); the residual work is in same-owner small-page full/active page-list transition cost and benchmark-row variance, not large/huge unmapping.

## Next

- [patch] Complete the Miri page-metadata provenance fix before resuming the
  RpMalloc deallocation-gap investigation.
