//! Tests for the nightly-rustc release-channel classifier.

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
