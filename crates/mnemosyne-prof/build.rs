//! Thin caller into the shared nightly-rustc probe (`mnemosyne-build-util`),
//! which owns the `nightly_tls_active` cfg emission end to end.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    mnemosyne_build_util::emit_nightly_tls_cfg();
}
