//! Build script that decides whether a jemalloc comparator is available and,
//! on Windows, links a system-installed `libjemalloc_s.a`. The
//! `nightly_tls_active` cfg is emitted by the shared probe in
//! `mnemosyne-build-util` (SSOT), not here.
//!
//! - Non-Windows targets get jemalloc through the `tikv-jemallocator`
//!   dependency, so `jemalloc_available` is emitted unconditionally.
//! - Windows targets get jemalloc only when the `system-jemalloc` feature is
//!   enabled and a static `libjemalloc_s.a` is found, because
//!   `tikv-jemallocator` builds jemalloc from source and does not link on
//!   windows-gnu. The library is located from `MNEMOSYNE_JEMALLOC_LIB_DIR` or
//!   by scanning `PATH` for an MSYS2-style `*/{ucrt64,mingw64}/bin` entry and
//!   checking its sibling `lib/` directory.

use std::env;
use std::path::PathBuf;

fn find_jemalloc_lib_dir() -> Option<PathBuf> {
    if let Ok(dir) = env::var("MNEMOSYNE_JEMALLOC_LIB_DIR") {
        let p = PathBuf::from(dir);
        if p.join("libjemalloc_s.a").exists() {
            return Some(p);
        }
    }
    let path = env::var_os("PATH")?;
    for entry in env::split_paths(&path) {
        // MSYS2 layout: `.../ucrt64/bin` (or `mingw64/bin`) has a sibling
        // `.../ucrt64/lib` holding `libjemalloc_s.a`.
        if entry.file_name().is_some_and(|n| n == "bin")
            && let Some(parent) = entry.parent()
        {
            let lib = parent.join("lib");
            if lib.join("libjemalloc_s.a").exists() {
                return Some(lib);
            }
        }
    }
    None
}

fn main() {
    println!("cargo::rustc-check-cfg=cfg(jemalloc_available)");
    println!("cargo::rerun-if-env-changed=MNEMOSYNE_JEMALLOC_LIB_DIR");
    println!("cargo::rerun-if-changed=build.rs");

    mnemosyne_build_util::emit_nightly_tls_cfg();

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        // `tikv-jemallocator` is a non-Windows dependency and supplies jemalloc.
        println!("cargo::rustc-cfg=jemalloc_available");
        return;
    }

    // Windows: opt-in only.
    if env::var_os("CARGO_FEATURE_SYSTEM_JEMALLOC").is_none() {
        return;
    }

    match find_jemalloc_lib_dir() {
        Some(dir) => {
            println!("cargo::rustc-link-search=native={}", dir.display());
            println!("cargo::rustc-link-lib=static=jemalloc_s");
            println!("cargo::rustc-cfg=jemalloc_available");
        }
        None => {
            println!(
                "cargo::warning=system-jemalloc feature enabled but libjemalloc_s.a was not \
                 found (scanned PATH for */{{ucrt64,mingw64}}/bin siblings). The jemalloc \
                 benchmark column will be skipped. Set MNEMOSYNE_JEMALLOC_LIB_DIR to override."
            );
        }
    }
}
