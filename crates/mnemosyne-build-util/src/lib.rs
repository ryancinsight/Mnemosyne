//! Internal build-script utility: the single authoritative nightly-rustc
//! detection probe.
//!
//! Several crates gate a nightly-only `#[thread_local]` fast path behind the
//! `nightly_tls` cargo feature plus a build-time probe of the active `rustc`.
//! The probe logic lives here once; consumer `build.rs` scripts are thin
//! callers. This crate is consumed only through `[build-dependencies]` —
//! never from library or binary code.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::env;
use std::process::Command;

/// Emits the `nightly_tls_active` cfg for the calling build script.
///
/// Behavior (identical for every consumer):
/// 1. Declares `cargo::rustc-check-cfg=cfg(nightly_tls_active)` so the cfg is
///    always known to the lint machinery, active or not.
/// 2. Declares `cargo::rerun-if-env-changed=RUSTC` so switching toolchains
///    re-runs the probe.
/// 3. When the consuming crate's `nightly_tls` cargo feature is enabled
///    (`CARGO_FEATURE_NIGHTLY_TLS` is set) and `$RUSTC -vV` reports a
///    `release:` line containing `nightly`, emits
///    `cargo::rustc-cfg=nightly_tls_active`.
///
/// A missing or failing `rustc` invocation leaves the cfg inactive: the
/// consumer then compiles its stable (non-`#[thread_local]`) path, which is
/// correct on every toolchain — this is capability detection, not an error
/// fallback.
pub fn emit_nightly_tls_cfg() {
    println!("cargo::rustc-check-cfg=cfg(nightly_tls_active)");
    println!("cargo::rerun-if-env-changed=RUSTC");

    if env::var_os("CARGO_FEATURE_NIGHTLY_TLS").is_none() {
        return;
    }
    if rustc_is_nightly() {
        println!("cargo::rustc-cfg=nightly_tls_active");
    }
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
mod tests {
    use super::release_is_nightly;

    #[test]
    fn nightly_release_line_is_detected() {
        let out = "rustc 1.90.0-nightly (abcdef123 2026-06-30)\n\
                   binary: rustc\n\
                   commit-hash: abcdef123\n\
                   release: 1.90.0-nightly\n\
                   host: x86_64-pc-windows-gnu\n";
        assert!(release_is_nightly(out));
    }

    #[test]
    fn stable_release_line_is_not_nightly() {
        let out = "rustc 1.88.0 (deadbeef 2026-05-01)\nrelease: 1.88.0\n";
        assert!(!release_is_nightly(out));
    }

    #[test]
    fn beta_release_line_is_not_nightly() {
        let out = "release: 1.89.0-beta.3\n";
        assert!(!release_is_nightly(out));
    }

    #[test]
    fn nightly_outside_release_line_is_ignored() {
        // `nightly` appearing in another line (e.g. commit description) must
        // not trigger detection; only the `release:` channel counts.
        let out = "rustc 1.88.0 (nightly-fix backport)\nrelease: 1.88.0\n";
        assert!(!release_is_nightly(out));
    }

    #[test]
    fn missing_release_line_is_not_nightly() {
        assert!(!release_is_nightly("binary: rustc\n"));
        assert!(!release_is_nightly(""));
    }
}
