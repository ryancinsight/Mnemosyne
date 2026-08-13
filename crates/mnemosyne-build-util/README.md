# mnemosyne-build-util

Internal build-script utility for the
[Mnemosyne](https://github.com/ryancinsight/mnemosyne) allocator workspace.

```toml
[build-dependencies]
mnemosyne-build-util = "0.2"
```

Several Mnemosyne crates gate a nightly-only `#[thread_local]` fast path behind
the `nightly_tls` cargo feature plus a build-time probe of the active `rustc`.
This crate is the one place that probe is implemented; consumer `build.rs`
scripts are thin callers, so the detection logic cannot drift between them.

The single entry point emits the `nightly_tls_active` cfg for the calling build
script.

This crate is consumed only through `[build-dependencies]`. It is not intended
for use from library or binary code, and it declares
`#![forbid(unsafe_code)]`.

Licensed under MIT OR Apache-2.0.
