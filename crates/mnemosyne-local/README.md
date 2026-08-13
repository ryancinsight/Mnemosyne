# mnemosyne-local

Thread-local allocation engine for the
[Mnemosyne](https://github.com/ryancinsight/mnemosyne) allocator.

```toml
[dependencies]
mnemosyne-local = "0.4"
```

`ThreadAllocator` owns the per-thread fast path: size-class routing, the
page-local free list, page refill, page recycling across size classes, and
segment reclamation at thread exit. Local frees never touch an atomic; remote
frees are drained from the page's cross-thread queue in a batch, and only after
the local list is exhausted.

Invariants that hold structurally in `alloc`, `alloc_cold`, `get_new_page`, and
`try_recycle_page` compile to `debug_assert!` plus
`core::hint::unreachable_unchecked()`, so the release hot path stays branch-free
while debug builds keep full validation.

## `nightly_tls`

The default build reads the per-thread cache slot through `std::thread_local!`
with a `const {}` initializer — the fastest stable accessor, though it still
lowers to a `LocalKey::with` call. The optional `nightly_tls` feature switches to
an unstable `#[thread_local]` static, which compiles to a single
segment-register-relative load. Thread-exit segment reclamation, which a
`#[thread_local]` static does not run, is preserved by a `std::thread_local!`
`Drop` sentinel armed once per thread off the hot path.

The feature requires a nightly compiler and is off by default; the stable build
is unchanged.

Licensed under MIT OR Apache-2.0.
