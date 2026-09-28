//! Internal build-script utility: the single authoritative nightly-rustc
//! detection probe.
//!
//! Several crates gate a nightly-only `#[thread_local]` fast path behind the
//! `nightly_tls` cargo feature plus a build-time probe of the active `rustc`.
//! The probe logic lives here once; consumer `build.rs` scripts print the
//! directives this crate returns. This crate is consumed only through
//! `[build-dependencies]` — never from library or binary code.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::env;
use std::process::Command;

/// Returns the `cargo::` directives the calling build script must print.
///
/// Behavior (identical for every consumer):
/// 1. `rustc-check-cfg=cfg(nightly_tls_active)` so the cfg is always known to
///    the lint machinery, active or not.
/// 2. `rerun-if-env-changed=RUSTC` so switching toolchains re-runs the probe.
/// 3. When the consuming crate's `nightly_tls` cargo feature is enabled
///    (`CARGO_FEATURE_NIGHTLY_TLS` is set) and `$RUSTC -vV` reports a
///    `release:` line containing `nightly`,
///    `rustc-cfg=nightly_tls_active`.
///
/// The caller prints each entry as `println!("cargo::{directive}")`. Keeping
/// the I/O in the build script keeps this crate a pure decision function and
/// keeps the cargo protocol channel at the build-script boundary.
///
/// A missing or failing `rustc` invocation leaves the cfg inactive: the
/// consumer then compiles its stable (non-`#[thread_local]`) path, which is
/// correct on every toolchain — this is capability detection, not an error
/// fallback.
#[must_use]
pub fn nightly_tls_directives() -> Vec<&'static str> {
    let mut directives = vec![
        "rustc-check-cfg=cfg(nightly_tls_active)",
        "rerun-if-env-changed=RUSTC",
    ];
    if env::var_os("CARGO_FEATURE_NIGHTLY_TLS").is_some() && rustc_is_nightly() {
        directives.push("rustc-cfg=nightly_tls_active");
    }
    directives
}

/// Runs `$RUSTC -vV` (falling back to `rustc` when the env var is unset, as
/// during a direct `rustc` invocation outside cargo) and classifies the
/// release channel.
fn rustc_is_nightly() -> bool {
    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let Ok(output) = Command::new(rustc).arg("-vV").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    release_is_nightly(&String::from_utf8_lossy(&output.stdout))
}

/// Classifies `rustc -vV` output: nightly iff the `release:` line contains
/// `nightly`. Beta/stable/dev channels and malformed output are not nightly.
fn release_is_nightly(version_output: &str) -> bool {
    version_output.lines().any(|line| {
        line.strip_prefix("release: ")
            .is_some_and(|release| release.contains("nightly"))
    })
}

#[cfg(test)]
mod tests;
