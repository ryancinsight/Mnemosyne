# Mnemosyne: A High-Performance User-Space Memory Allocator in Rust

Mnemosyne—named after the Greek goddess of memory—is a high-performance concurrent memory allocator implemented completely in Rust. It utilizes a deep vertical hierarchical multi-crate workspace layout to enforce the Separation of Concerns (SoC), Single Responsibility Principle (SRP), Single Source of Truth (SSOT), Dependency Inversion Principle (DIP), and Don't Repeat Yourself (DRY) paradigms.

Its design incorporates core lessons from modern allocator research (specifically **mimalloc** and **snmalloc**), implementing thread-local fast-path caches, lock-free contention-free cross-thread message queues, and zero-cost compile-time allocation safety policies.

---

## Architectural Highlights

### 1. Zero-Cost Compile-Time Safety Policies (`AllocPolicy`)
*   **Compile-Time Configuration**: Parameterized via Zero-Sized Types (ZSTs) and a sealed `AllocPolicy` trait. This shifts optimization branch decisions to compile-time (dead code elimination), carrying absolute zero runtime performance cost.
*   **Backwards Compatibility**: The standard `Mnemosyne` global allocator routes allocations to `StandardPolicy` by default, preserving direct static initialization (e.g. `static ALLOCATOR: Mnemosyne = Mnemosyne;`).
*   **Secure Routing**: The generic `MnemosyneAllocator<P: AllocPolicy>` allows static injection of safety layers such as `SecurePolicy`, which guarantees zero-initialization on allocation and memory poisoning (`0xDE` write pattern) at the deallocation boundary.
*   **Inline Pointer Protection**: Poisoning of freed blocks occurs strictly before they are linked into the free list, avoiding next-pointer corruption inside the inline payload.

### 2. Global Allocator Shell and Heap Branding Boundary
*   The top-level `mnemosyne` crate exposes the global allocator, policy types, telemetry, and branded ownership APIs. Explicit isolated heaps remain in the `mnemosyne-heap` crate instead of being presented as a second allocator choice from the shell crate.
*   `MnemosyneHeap` and `BrandedHeap` share one internal `RawHeap<P, B>` implementation for allocation, free, and realloc mechanics. Branding adds type-level ownership evidence at the API boundary; it does not clone allocator algorithms.
*   Use the global allocator for process-wide allocation, branded heaps when lifetime branding improves ownership stability, and explicit heaps only for isolated allocation streams or targeted tests/benchmarks.

### 3. Contention-Free Cross-Thread Free Queueing (Snmalloc Style)
*   Cross-thread and re-entrant frees route through each owning page's atomic free queue rather than contending on page locks, central pools, or an allocator-level incoming queue.
*   Reclamation of remote frees is batched and executed strictly after local free lists are exhausted, preserving the hot allocation path while keeping page ownership explicit.

### 4. Orphaned Segment Adoption & Reuse
*   When a thread terminates, its active segments are not immediately returned to the OS. Partially occupied segments are pushed to a lock-free `GLOBAL_ORPHAN_POOL`.
*   Active threads seeking new pages scan this pool and adopt orphaned segments, scanning for empty pages to repurpose (recycling them across different size classes) and resuming allocations from partially filled pages, eliminating address-space leaks.

### 5. Zero-Panic Library Assurance
*   The production library crates (`mnemosyne-core`, `mnemosyne-arena`, `mnemosyne-backend`, `mnemosyne-local`) are completely free of `.unwrap()`, `.expect()`, and explicit `panic!` pathways, ensuring absolute runtime stability under memory constraints.
*   Structurally guaranteed invariants in `ThreadAllocator::alloc`, `alloc_cold`, `get_new_page`, and `try_recycle_page` compile down to `debug_assert!` + `core::hint::unreachable_unchecked()`, keeping the release-mode hot path branch-free while preserving full debug-build validation.

### 6. Centralized Allocation Request Validation
*   `mnemosyne-core::validation` exposes two `const fn` predicates — `is_valid_alloc_request` for unsafe direct entry points and `is_valid_layout_alloc_request` for `Layout`-validated `GlobalAlloc::alloc` callers.
*   Every allocation entry point (`thread_alloc`, `thread_alloc_layout`, `allocate_large_or_huge`) routes through the same single-source predicates, so a change to `MAX_ALLOC_SIZE`, the power-of-two alignment requirement, or the `SEGMENT_SIZE` alignment cap edits exactly one definition while monomorphization keeps every call site branch-for-branch identical to the prior inlined checks.

### 7. Cache-Line-Aligned Page Metadata
*   `Page` is exactly 64 bytes — one cache line on 64-bit targets — and the layout is pinned by `page_struct_size_stays_within_one_cache_line`. Every fast-path allocation reads and writes `page.free`, `page.local_free`, `page.alloc_count`, and `page.block_size` from a single contiguous cache line.
*   The dead `Page::segment` back-pointer field was removed: callers always recover the parent segment by rounding the page address down to `SEGMENT_ALIGN`, eliminating 32 stores per fresh segment initialization.

### 8. Backend Release Accounting
*   `MemoryBackend::deallocate` returns a release-success boolean and is marked `#[must_use]`. `MemoryBackendWrapper` defers `current_mapped_bytes` decrements to confirmed OS release and routes failures through a dedicated `record_unmap_failure` path that increments only the call counter, so a failed `munmap`/`VirtualFree` cannot leave the telemetry counter under-counting still-mapped bytes.
*   `purge_segment_pool` counts only confirmed releases as purged and pushes failed releases back into the retained pool, preserving ownership metadata on backend failure.

### 9. Transparent Huge Page Hint (Linux)
*   `UnixBackend::allocate` issues `madvise(MADV_HUGEPAGE)` on Linux for mappings that are at least one full `SEGMENT_SIZE` (2 MiB) and a multiple thereof. The kernel can then back each segment with a single 2 MiB transparent huge page, halving TLB pressure on hot segment-metadata access.
*   The advice is purely advisory and ignored on kernels without THP support; the mapping itself is never invalidated by a hint failure. Non-Linux Unix targets compile a no-op stub.

### 10. Page-Level OS Reclaim (`page_reset` and `reset()`)
*   `MemoryBackend::page_reset(ptr, size) -> bool` lets a backend release the physical backing of an idle page range while keeping the virtual mapping committed: `MADV_DONTNEED` on Linux, `MADV_FREE` on macOS/FreeBSD, `VirtualAlloc(MEM_RESET)` on Windows. The default trait impl returns `false` so backends without an equivalent operation silently opt out.
*   `mnemosyne::reset()` drives `reset_segment_pool` which drains the retained free-segment pool, asks the backend to drop the physical backing of each cached segment's mapping, and pushes the segments back into the cache so the address-space stays warm.
*   `MemoryBackendWrapper` records `page_reset_calls` and `page_reset_bytes` telemetry on confirmed resets; the arena pool tracks `reset_calls` and `reset_segments` separately. Neither path decrements `current_mapped_bytes` - the virtual mapping remains owned by the allocator and only the resident set drops.
*   This complements `purge()` (which releases both address space and RSS) as a lighter-weight RSS-reduction knob for idle periods.

### 11. Segment-Tail Guard Region (`make_guard`)
*   `MemoryBackend::make_guard(ptr, size) -> bool` installs a `PROT_NONE` (Unix `mprotect`) or `PAGE_NOACCESS` (Windows `VirtualProtect`) region inside an active mapping. The address range stays reserved and is releasable via `deallocate`, but any read or write raises an access-violation fault.
*   The opt-in `mnemosyne-arena/segment-tail-guards` feature installs a 4 KiB guard at `aligned_addr + SEGMENT_SIZE` on every fresh OS-backed segment, inside the alignment slack the arena already reserves. Forward OOB writes that walk past the last user page (Page 31) trap on the guard instead of corrupting an unrelated mapping.
*   `MemoryBackendWrapper` records `guard_install_calls` and `guard_install_bytes` telemetry on confirmed installs and intentionally does not decrement `current_mapped_bytes`. The install is best-effort: backends without `make_guard` support (default impl) or kernels with an OS page size larger than the guard size (macOS-arm64) silently skip without affecting correctness.
*   The default feature set leaves segment-tail guards disabled so production builds and benchmark runs keep zero guard-install overhead.

### 12. `usable_size` API and In-Place `realloc`
*   `mnemosyne::usable_size(ptr)` returns the allocator's actual reservation for a previously-allocated pointer — the size-class block size for small allocations (which may exceed the original request because Mnemosyne rounds up to the next class), the distance from the user pointer to the end of the payload mapping for huge allocations, and 0 for null. Mirrors `mi_usable_size` (mimalloc) and `malloc_usable_size` (glibc/jemalloc).
*   `Mnemosyne` and `MnemosyneAllocator<P, B>` override `GlobalAlloc::realloc` to consult `usable_size(ptr)` first and return the same pointer unchanged when the new size fits inside the existing size-class block. This eliminates the alloc/copy/free round trip on the common `Vec::push` capacity-rounding case. Secure policies keep replacement allocation on growth so new bytes are zero-initialized.
*   Small-allocation probes read the target page's size-class metadata directly and fall back to the huge-allocation metadata slot only for uninitialized large/huge pages.

### 13. Tight Huge-Allocation Mapping Derivation
*   `allocate_large_or_huge` reserves exactly `size + alignment + SEGMENT_ALIGN + PAGE_SIZE` from the backend, derived from a four-step layout walk over the worst-case slacks (segment-alignment round-up, page-zero reserved prefix, payload-alignment round-up, payload). The prior derivation over-reserved by an entire `SEGMENT_SIZE`, wasting ~2 MiB − 64 KiB of mapped memory per huge allocation; the tight formula is pinned by `huge_allocation_consumes_tight_mapping_size` which asserts the exact backend telemetry delta.
*   Power-of-two alignments above `SEGMENT_ALIGN` are rejected at the entry point so that free classification can always recover the segment header by segment rounding or metadata-slot lookup, without a side registry.

### 14. C ABI Shim (`mnemosyne-c-shim`)
*   The `mnemosyne-c-shim` crate exposes `malloc`, `free`, `calloc`, `realloc`, `aligned_alloc`, `posix_memalign`, and `malloc_usable_size` as `#[no_mangle] extern "C"` functions. Built as both `lib` (for Rust consumers) and `cdylib` (for `LD_PRELOAD` on Unix / DLL injection on Windows), it lets C/C++ code or whole processes use Mnemosyne without a Rust `#[global_allocator]`.
*   C `free`/`realloc`/`malloc_usable_size` are pointer-only — no `Layout` is threaded through — which Mnemosyne supports natively because the page/segment owner is recovered by address rounding. The shim's `realloc` copies `min(usable_size, new_size)` to honor C semantics (where the caller may have written the entire usable region), deliberately distinct from the Rust `GlobalAlloc::realloc` path's `layout.size()` bound.
*   A matching C declaration header ships at [`crates/mnemosyne-c-shim/include/mnemosyne.h`](file:///d:/Mnemosyne/crates/mnemosyne-c-shim/include/mnemosyne.h) for C/C++ consumers, documenting the per-function null/zero/overflow/alignment contracts.

### 15. Scratch Pool Element Contract
*   `mnemosyne-arena::scratch::ScratchPool<T>` and the top-level `mnemosyne::scratch` re-export support `f32`, `f64`, and `u8` unconditionally.
*   Complex scratch lanes use the Atlas numeric SSOT: enable the `eunomia` feature and use `eunomia::Complex<f32>` or `eunomia::Complex<f64>`. Mnemosyne no longer exposes an internal `num-complex` compatibility feature; local Atlas consumers were audited and no remaining `mnemosyne/num-complex` user was found.

### 16. By-Concern Backend Module Layout (`mnemosyne-backend`)
*   `mnemosyne-backend` is organized by backend responsibility:
    *   [`mapping`](crates/mnemosyne-backend/src/mapping.rs) owns the `MemoryBackendWrapper` struct shape and the single central `impl MemoryBackend for MemoryBackendWrapper` block. Rust's trait coherence rule keeps the `impl` in one file, and the `allocate` / `deallocate` bodies live inline here. `make_guard`, `page_reset`, and `decommit` entries delegate to the per-concern helpers in `guard` and `reset` via `#[inline(always)]` static-dispatch calls.
    *   [`guard`](crates/mnemosyne-backend/src/guard.rs) owns `do_make_guard` — the per-method body the `make_guard` entry in `mapping`'s impl block delegates into. The `wrapper_make_guard_records_confirmed_install_and_keeps_mapping_reserved` and `wrapper_make_guard_rejects_null_and_zero` tests anchor the guard concern end-to-end.
    *   [`reset`](crates/mnemosyne-backend/src/reset.rs) owns `do_page_reset` and `do_decommit` — the per-method bodies the `page_reset` and `decommit` entries in `mapping`'s impl block delegate into. The `wrapper_page_reset_round_trips_on_active_mapping`, `wrapper_page_reset_rejects_null_and_zero`, and `wrapper_decommit_returns_slack_and_keeps_reservation_releasable` tests anchor the reset concern end-to-end.
    *   [`recorders`](crates/mnemosyne-backend/src/recorders.rs) owns the telemetry counter statics, the `BackendMemoryStats` snapshot, and the per-concern unit tests for the `record_*` family. Counter updates are `pub(crate)` so sibling concern modules can record confirmed outcomes; external consumers reach the snapshot through `backend_memory_stats()`.
    *   [`backends`](crates/mnemosyne-backend/src/backends/mod.rs) owns the per-OS / per-platform backend implementations: `UnixBackend`, `WindowsBackend`, `CudaUnifiedBackend`, `CudaDeviceBackend`, `CudaHostPinnedBackend`, and `WgpuStagingBackend`. `DefaultBackend` selects the OS-conditional backing per the active platform at compile time.
*   Public re-exports at the crate root keep the canonical `mnemosyne_backend::CudaUnifiedBackend`, `CudaDeviceBackend`, `CudaHostPinnedBackend`, `MemoryBackendWrapper`, `WgpuStagingBackend`, `DefaultBackend`, and `backend_memory_stats` paths; backend-specific helpers live under `backends` and are reached by qualified path.
*   `WgpuStagingBackend` uses private process-global callback slots. Consumers install callbacks through `register_wgpu_callbacks(WgpuAllocateCallback, WgpuDeallocateCallback)`, an unsafe typed registration boundary that rejects mismatched callback signatures at compile time and keeps raw pointer storage out of the public API.
*   The trait-coherence-required central `impl` block stays in `mapping`; the per-concern helpers (`do_make_guard`, `do_page_reset`, `do_decommit`) carry the `pub(crate)` boundary so external consumers reach the public `MemoryBackend` trait surface only. Evidence tier: source-level static dispatch plus backend unit tests and allocator benchmark threshold gates.

---

## Research Foundations

Mnemosyne's design is grounded in the modern allocator literature. Each
implemented technique below names the paper or production allocator it
derives from, so the codebase can be read against its sources.

| Mnemosyne mechanism | Source | Where in the code |
| :--- | :--- | :--- |
| Per-page sharded free lists (`free` + `thread_free`), local frees never touch atomics | mimalloc — *Free List Sharding in Action*, MSR-TR-2019-18 | `mnemosyne-core::types::Page`, `ThreadAllocator::alloc` |
| Page-local lock-free cross-thread free queue, batch-reclaimed after the local list drains | snmalloc — *snmalloc: A Message Passing Allocator*, ISMM 2019 | `AtomicFreeList`, `Page::reclaim_thread_free` |
| 2 MiB segments sliced into 64 KiB pages; segment header recovered by address rounding (no side table) | mimalloc segment/page geometry | `mnemosyne-arena::segment`, `thread_free` classifier |
| Orphaned-segment adoption on thread exit instead of eager OS release | snmalloc abandoned-slab adoption / mimalloc abandoned pages | `GLOBAL_ORPHAN_POOL`, `ThreadAllocator::Drop` |
| Decay-style RSS reduction without surrendering the address space (`reset()` → `page_reset`) | jemalloc decay purging; mimalloc page reset | `mnemosyne::reset`, `reset_segment_pool`, `MemoryBackend::page_reset` |
| Transparent Huge Page hint for segment-aligned mappings | jemalloc THP, mimalloc large-page | `UnixBackend::allocate` (`MADV_HUGEPAGE`) |
| Guard regions (`PROT_NONE` / `PAGE_NOACCESS`) for OOB-write trapping | hardened_malloc, Scudo, PartitionAlloc | `MemoryBackend::make_guard`, segment-tail guard |
| Compile-time policy ZSTs for zero-cost secure-vs-standard selection | mimalloc-secure build flag, lifted to the Rust type system | `AllocPolicy`, `SecurePolicy`, `StandardPolicy` |

The external gap analysis in [`gap_analysis_external.md`](file:///d:/Mnemosyne/gap_analysis_external.md)
tracks which further research techniques (free-list pointer encryption,
NUMA-aware arenas, per-CPU caching via `rseq`, Mesh-style compaction) are
candidates and which are deliberately out of scope, with each row carrying
a priority tag and a named test guard.

**Performance positioning (honest standing).** Against the bounded
Criterion comparison set, Mnemosyne leads snmalloc and the system
allocator across every group, and matches or beats mimalloc on threaded,
cross-thread, burst, and large/medium cycle workloads. The one open
deficit is single-threaded *small* allocation-cycle latency versus
mimalloc; evidence (recorded in `gap_audit.md`) localizes that gap to the
thread-local slot accessor rather than the allocation algorithm itself.
Closing it is gated on a quiescent benchmark environment capable of
arbitrating sub-nanosecond deltas — hot-path changes are merged only when
a clean measurement justifies them, and rejected experiments are logged
rather than carried.

**Fastest-possible thread-local storage (`nightly_tls`).** The default build
uses `std::thread_local!` with a `const {}` initializer — the fastest *stable*
accessor, but one that still lowers to a `LocalKey::with` call. The optimal
mechanism is the one mimalloc uses for its default heap: an ELF/PE
`#[thread_local]` variable, which compiles to a single segment-register-relative
load with no call and no initialization guard. Rust exposes this via the
unstable `#[thread_local]` attribute, so it ships as the opt-in
`mnemosyne-local/nightly_tls` feature (requires a nightly compiler). When
enabled, the per-thread cache slot becomes a `#[thread_local]` static accessed
directly; thread-exit segment reclamation — which a `#[thread_local]` static
does not run automatically — is preserved by a `std::thread_local!` `Drop`
sentinel armed once per thread off the hot path. The default stable build is
unchanged and byte-identical. The mechanism is verified for correctness
(including a spawned-thread reclamation test); confirming the predicted
single-load latency win, and whether to default the feature on nightly build
targets, is gated on the same quiescent benchmark environment noted above.

---

## Multi-Crate Workspace Layout

The project resides in a deep vertical module hierarchy:

```mermaid
graph TD
    Shell[mnemosyne] --> Local[mnemosyne-local]
    Local --> Arena[mnemosyne-arena]
    Arena --> Core[mnemosyne-core]
    Arena --> Backend[mnemosyne-backend]
    Backend --> Core
```

*   **[mnemosyne](file:///d:/Mnemosyne/crates/mnemosyne)**: The public shell global allocator interface and telemetry endpoints.
*   **[mnemosyne-local](file:///d:/Mnemosyne/crates/mnemosyne-local)**: Thread-local cache (`ThreadAllocator`) and size-class fast-path routing.
*   **[mnemosyne-arena](file:///d:/Mnemosyne/crates/mnemosyne-arena)**: Global aligned segment management, page slicing, and orphan pools.
*   **[mnemosyne-backend](file:///d:/Mnemosyne/crates/mnemosyne-backend)**: Page allocation adapter mapping to virtual memory primitives (`VirtualAlloc`/`VirtualFree` on Windows; `mmap`/`munmap` on Unix).
*   **[mnemosyne-core](file:///d:/Mnemosyne/crates/mnemosyne-core)**: Shared size-class logic, atomic collections, constants, and compile-time policies.
*   **[mnemosyne-benchmarks](file:///d:/Mnemosyne/crates/mnemosyne-benchmarks)**: Criterion performance harness and memory usage report utilities.

---

## Usage Guide

To register Mnemosyne as your global allocator using the default, high-performance `StandardPolicy`:

```rust
use mnemosyne::Mnemosyne;

#[global_allocator]
static ALLOCATOR: Mnemosyne = Mnemosyne;

fn main() {
    let x = Box::new(42);
    assert_eq!(*x, 42);
}
```

To use the compile-time `SecurePolicy` (zero-initialization and freed payload poisoning):

```rust
use mnemosyne::{MnemosyneAllocator, SecurePolicy};

#[global_allocator]
static ALLOCATOR: MnemosyneAllocator<SecurePolicy> = MnemosyneAllocator::new();

fn main() {
    let x = Box::new(42);
    assert_eq!(*x, 42);
}
```

To read allocator telemetry mapping, purging, and thread caching stats at runtime:

```rust
use mnemosyne::memory_stats;

fn main() {
    let stats = memory_stats();
    println!("Mapped Bytes: {}", stats.current_mapped_bytes);
    println!("Peak Mapped Bytes: {}", stats.peak_mapped_bytes);
    println!("Purged Segments: {}", stats.purged_segments);
}
```

To programmatically configure the allocator runtime options (e.g., to control background decay and segment retention):

```rust
use mnemosyne::{configure, get_options, MnemosyneOptions};

fn main() {
    // Configure options at runtime
    configure(MnemosyneOptions {
        max_retained_segments: 128,
        purge_cadence_ms: 100,
        enable_hugepage_hint: true,
    });

    let opt = get_options();
    assert_eq!(opt.max_retained_segments, 128);
}
```

To use scoped lifetime-branded memory allocation (GhostCell-style permission and data separation with `BrandedHeap`, `BrandedBox`, `BrandedVec`, and `BrandedCell`). The brand identity and the thread-confined capability token are provided by the [`melinoe`](https://github.com/ryancinsight/melinoe) crate — its `InvariantLifetime` marker and `ThreadLocalToken` are the single source of brand machinery for the Mnemosyne ecosystem:

```rust
use mnemosyne::{branded_scope, StandardPolicy};

fn main() {
    branded_scope::<StandardPolicy, _, _, _>(|heap, mut token| {
        // Allocate and initialize a single value
        let bbox = heap.alloc_init(&token, 42).unwrap();
        assert_eq!(*bbox, 42);

        // Or create a branded vector
        let mut vec = mnemosyne::BrandedVec::new(&heap);
        vec.push(&mut token, 10).unwrap();
        vec.push(&mut token, 20).unwrap();

        // Convert the vector to a shared cell for interior mutability
        let cell = vec.into_cell(&mut token);
        assert_eq!(cell.borrow(&token), &[10, 20]);
        
        // Mutate inside a borrow
        cell.borrow_mut(&mut token)[1] = 99;
        assert_eq!(cell.borrow(&token), &[10, 99]);

        // Reclaim memory safely
        heap.free(&mut token, unsafe { cell.into_block() });
    });
}
```

---

## Verification & Benchmarks

### Running Tests
Execute the workspace unit and integration tests:
```bash
cargo nextest run --workspace
cargo test --doc --workspace
```

### Running the Memory Report
Execute the memory report scenario verifying segment eviction bounds and manual pool purging:
```bash
cargo run -p mnemosyne-benchmarks --bin memory_report --release
```

### Running the Performance Benchmarks
To compare Mnemosyne, the system allocator, MiMalloc, and SnMalloc performance across allocation-only latency, deallocation-only latency, allocation/deallocation cycle latency, realloc latency, bursts, combined usable-size probes, isolated usable-size metadata queries, small and medium threaded cycles, and saturated threaded cycles:
```bash
# Run Criterion microbenchmarks
cargo bench -p mnemosyne-benchmarks --bench allocator_bench

# Extract estimates and generate side-by-side comparison report
cargo run -p mnemosyne-benchmarks --bin benchmark_summary --release

# Enforce selected Mnemosyne regression thresholds against the baseline excerpt
cargo run -p mnemosyne-benchmarks --bin benchmark_summary --release -- --enforce-thresholds
```

The `Threaded small allocation cycles` group preserves the historical four-worker measurement with one bounded-channel command per Criterion sample. The `Threaded medium allocation cycles` group repeats that topology with 1024-byte allocations. The `Threaded saturated small allocation cycles` group uses the same workers with a larger per-command allocation count, reducing benchmark coordination overhead relative to allocator work.
Only the saturated threaded row is included in the selected threshold baseline; the small and medium threaded rows remain visible in comparison tables as continuity and size-class disparity signals.
The source-controlled baseline is `benchmarks/allocator_baseline_excerpt.csv`; it changes only when `benchmark_summary` is run with `--refresh-baseline`. Current-run generated outputs live under `target/criterion/` (`benchmark_summary.csv`, `allocator_current_excerpt.csv`, `benchmark_baseline_comparison.csv`, `benchmark_variance.csv`, and `benchmark_metadata.json`) and are refreshed by normal summary runs. Threshold enforcement fails if any selected baseline row is missing from the current Criterion data. Threaded and cross-thread variance rows use the scheduler-aware `0.25` relative-width threshold; scalar latency rows use `0.15`.
The memory report includes page-refill, recycle, fresh-page, fresh-segment, orphan-adoption, and recycle-sweep counters so cold-path allocation behavior can be checked without adding hot-path atomics.
Benchmark runner contract failures print explicit `benchmark failure: <context>: <detail>` diagnostics instead of assertion or channel unwrap panics.
Unsafe benchmark operations carry local safety comments for dynamic symbols, unchecked layouts, allocator calls, and segment-cache cycles.
The CUDA unified-memory backend uses a three-state initialization gate for race-free dynamic symbol resolution, tracks managed allocations in a fixed-size registry, and falls back to the host backend when CUDA is unavailable or registry capacity is exhausted.

For the latest source-visible side-by-side benchmark comparison table against competitor allocators, see `benchmarks/allocator_comparison.md`; regenerate it from current Criterion data with `benchmark_summary`.
The generated comparison table reports top-level `Mnemosyne`, explicit `mnemosyne_heap::MnemosyneHeap`, and scoped `BrandedHeap` rows separately; row classification is exact so explicit and branded heap measurements cannot overwrite the public allocator row.
