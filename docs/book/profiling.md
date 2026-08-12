# Profiling and Leak Detection

Mnemosyne provides built-in heap profiling and leak detection without
requiring external tools.

## Heap Profiling

`enable_profiling(interval)` activates the Poisson heap sampler with the
specified allocation interval:

```rust,ignore
mnemosyne::enable_profiling(512);  // sample every ~512 allocations on average
```

`dump_profile()` flushes the sampled allocation profile to stderr in a
format compatible with analysis tools.

`disable_profiling()` / `is_profiling_enabled()` toggle and query state.

## Leak Detection

`enable_leak_detector()` installs a per-allocation tracking hook:

```rust,ignore
mnemosyne::enable_leak_detector();
// ... run the suspect code ...
mnemosyne::dump_leaks();  // report all live allocations
mnemosyne::disable_leak_detector();
```

`dump_leaks()` writes all currently live allocation records to stderr,
including allocation size and (on supported platforms) a stack backtrace.

## Alloc/Free Hooks

For custom instrumentation, install a C-ABI callback:

```rust,ignore
mnemosyne::register_alloc_hook(my_alloc_callback);
mnemosyne::register_free_hook(my_free_callback);
```

Hooks are called on every allocation and deallocation respectively. They
must be async-signal-safe and must not re-enter the allocator.

## `MemoryStats`

`mnemosyne::memory_stats()` returns a `MemoryStats` snapshot (~30 fields)
covering:
- Mapped bytes and committed bytes
- Segment pool depth (normal and huge)
- Per-size-class live allocation counts and bytes
- NUMA node breakdown of mapped memory
- Decay engine statistics
