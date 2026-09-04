# Profiling and Leak Detection

Mnemosyne exposes allocation instrumentation through the `mnemosyne-prof`
runtime, re-exported by the `mnemosyne` crate. The runtime keeps the allocator
fast path inactive until a hook, sampler, or leak detector is enabled. Reports
are written to caller-provided paths, so file I/O stays outside allocation and
free operations.

## Heap Profiling

`enable_profiling(sample_interval)` activates the Poisson heap sampler with a
mean byte interval:

```rust,ignore
mnemosyne::enable_profiling(512 * 1024); // mean sampled bytes
```

The sampler debits each allocation by its size and draws the next interval
from an exponential distribution. The setting therefore controls sampled
bytes, not allocation count; large allocations reach the sampling boundary
sooner than small ones. Each resident sample retains its allocation size and
an interned stack identity.

`dump_profile(path)` resolves sampled stacks and writes folded-stack records to
the path in `stack1;stack2 bytes` format. It returns `std::io::Result<()>`;
the dump is an explicit diagnostic operation and is not performed on the
allocation hot path.

`disable_profiling()` / `is_profiling_enabled()` toggle and query state.

## Leak Detection

`enable_leak_detector()` tracks every allocation observed while it is active,
including its captured stack:

```rust,ignore
mnemosyne::enable_leak_detector();
// ... run the suspect code ...
let leaked = mnemosyne::dump_leaks("mnemosyne-leaks.txt")?;
mnemosyne::disable_leak_detector();
```

`dump_leaks(path)` writes the currently tracked live records, including the
allocation size and resolved stack where the platform can resolve it. It
returns `std::io::Result<usize>` with the number of records written. An
allocation made before the detector was enabled is not retroactively tracked;
freeing a tracked allocation removes it even if the detector has since been
disabled. The example uses `?` because the real API reports path and I/O
failures.

## Alloc/Free Hooks

For custom instrumentation, install an `unsafe extern "C"` callback receiving
the block pointer and byte size:

```rust,ignore
unsafe extern "C" fn on_alloc(ptr: *mut core::ffi::c_void, size: usize) {
    // Record `ptr` and `size` without allocating or blocking.
}

mnemosyne::register_alloc_hook(Some(on_alloc));
mnemosyne::register_free_hook(None); // remove a hook
```

Callbacks run synchronously on successful allocation and deallocation. The
runtime guards against recursive instrumentation: an allocation made by a
callback can proceed, but its nested callback is skipped. Keep callbacks
non-blocking and allocation-free so instrumentation does not perturb the
workload or introduce allocator re-entry. `None` unregisters the corresponding
callback.

## `MemoryStats`

`mnemosyne::memory_stats()` returns a `MemoryStats` snapshot for the default
`Mnemosyne` allocator (`StandardPolicy` plus the host backend). The snapshot
covers:

- mapped and peak-mapped address space, map/unmap calls, and confirmed page
  resets and guard installations;
- retained standard segments and retained huge blocks, including byte totals
  and purge/reset counters;
- calling-thread live allocations, owned segments, cross-thread reclamation,
  page refills split into recycled/fresh pages and fresh segments, orphan
  adoption, and recycle sweeps; and
- per-size-class occupancy for the calling thread.

This is allocator telemetry, not a resident-set measurement: mapped bytes can
remain after page backing is reset, while `page_reset_bytes` and
`purged_bytes` record physical-backing and mapping release events separately.
Use `memory_stats_generic::<P, B>()` when the measured allocator uses a
non-default policy or backend; a snapshot for the wrong policy/backend reads a
different allocator cache rather than inferring the active one.

`memory_stats_json()` serializes the same snapshot plus per-size-class bins for
external analysis. The JSON and text report paths are diagnostic boundaries;
they allocate and perform I/O outside the allocator hot path.

## Measurement hygiene

Call `warm_current_thread()` before a measurement window to move thread-local
allocator initialization out of the measured region. Then use
`memory_stats()` to separate allocation churn from retention:

- increasing `current_thread_live_allocations` indicates live ownership;
- increasing `fresh_segments` or `map_calls` indicates capacity acquisition;
- increasing `recycled_pages` with stable `fresh_segments` indicates reuse;
- `retained_free_bytes` and `retained_huge_bytes` show idle capacity still
  cached for reuse; and
- `reset()` drops standard-segment physical backing while retaining virtual
  ranges, whereas `purge()` releases retained standard and huge cache entries.

`decay()` runs the normal cross-backend reclamation and purge step. Use it at a
controlled quiescent boundary when measuring reclamation; do not put report
dumps, resets, or decay calls inside the operation being benchmarked.
