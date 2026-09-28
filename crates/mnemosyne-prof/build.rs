//! Thin caller into the shared nightly-rustc probe (`mnemosyne-build-util`),
//! which owns the `nightly_tls_active` cfg decision end to end.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    for directive in mnemosyne_build_util::nightly_tls_directives() {
        println!("cargo::{directive}");
    }
}
