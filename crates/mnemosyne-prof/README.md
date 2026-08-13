# mnemosyne-prof

Heap profiling runtime for the
[Mnemosyne](https://github.com/ryancinsight/mnemosyne) allocator.

```toml
[dependencies]
mnemosyne-prof = "0.2"
```

Three facilities, all reached through the `on_alloc` / `on_free` entry points
that the allocator crates call on every allocation and deallocation:

- user alloc/free trace hooks,
- a Poisson heap sampler (`dump_profile`, `Sample`, `StackId`),
- an every-allocation leak detector (`dump_leaks`).

Re-entrancy into the allocator from a hook is guarded by
`enter_hook`/`exit_hook`, so profiling an allocation cannot recurse into
profiling.

Licensed under MIT OR Apache-2.0.
