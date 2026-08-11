# Profiling and Leak Detection

`mnemosyne-prof` instruments every allocation and free at the thread-local
boundary, giving you a sampled view of where memory comes from and goes
without recompiling the application.  The sampler's internals are split
across six canonical leaves — `hasher`, `stack_interner`, `capture`,
`store`, `sampling`, and `report` (ADR 0004) — but the public contract is a
handful of functions.

## Enabling the sampler

```rust
use mnemosyne::enable_profiling;

enable_profiling(4096); // sample every 4096th allocation
```

`enable_profiling(sample_interval)` arms a Poisson sampler that records a
`Sample` for selected allocations:

```rust
pub struct Sample {
    pub size: usize,
    pub stack: StackId,
}
```

`StackId` is an interned backtrace identifier — the store keeps one copy of
each unique stack and references it by id, so the profiling overhead stays
bounded.  `disable_profiling()` and `is_profiling_enabled()` complete the
control surface, and `reset_profiler_for_testing()` is available from the
`mnemosyne_prof` crate for tests.  All of the control functions except the
testing-only reset are re-exported at the `mnemosyne` crate root.

## Hooks

If you need to observe every allocation rather than a sample, register
hooks (also at the crate root):

```rust
use mnemosyne::{register_alloc_hook, register_free_hook};

register_alloc_hook(Some(my_alloc_hook));
register_free_hook(Some(my_free_hook));
```

The hooks receive the pointer and the requested size on the thread-local
path, and `None` unregisters.  The C shim exposes the same surface as
`mnemosyne_register_alloc_hook` / `mnemosyne_register_free_hook` for native
consumers.

## Dumping a profile

```rust
use mnemosyne::dump_profile;

dump_profile("profile.txt")?;
```

`dump_profile(path: &str) -> std::io::Result<()>` writes the sampled
allocation sites — stack id, count, and bytes — to the given path.  The C
shim equivalent is `mnemosyne_dump_profile`.

## Leak detection

The leak detector tracks live blocks between alloc and free:

```rust
use mnemosyne::{dump_leaks, enable_leak_detector};

enable_leak_detector();
// ... application runs ...
let leaks = dump_leaks("leaks.txt")?;
```

`dump_leaks(path: &str) -> std::io::Result<usize>` writes the stacks of
blocks that were allocated but never freed and returns the number of leaked
sites.  `disable_leak_detector()` and `is_leak_detector_enabled()` round out
the API; the C shim mirrors all of it (`mnemosyne_enable_leak_detector`,
`mnemosyne_dump_leaks`, …).

## Recommended workflow

1. `enable_leak_detector()` in a test or debug build that exercises the
   subsystem.
2. Run the workload; call `dump_leaks` at the end and treat the count as a
   regression signal.
3. For allocation-site analysis, `enable_profiling` with a modest interval,
   run the workload, and `dump_profile` to see the hot sites.

Both tools work on the same thread-local instrumentation, so there is no
extra allocation bookkeeping to wire up — enabling them is the whole setup.
