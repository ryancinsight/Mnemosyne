use std::{env, process::Command};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(nightly_tls_active)");
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=RUSTC");

    if env::var_os("CARGO_FEATURE_NIGHTLY_TLS").is_none() {
        return;
    }

    let rustc = env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let Ok(output) = Command::new(rustc).arg("-vV").output() else {
        return;
    };
    if !output.status.success() {
        return;
    }

    let version = String::from_utf8_lossy(&output.stdout);
    if version.lines().any(|line| {
        line.strip_prefix("release: ")
            .is_some_and(|release| release.contains("nightly"))
    }) {
        println!("cargo::rustc-cfg=nightly_tls_active");
    }
}
