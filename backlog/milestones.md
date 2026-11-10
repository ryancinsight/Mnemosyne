# Milestone ladder ([ADR 0014](../docs/adr/0014-release-scope-1-0.md))
<a id="M-core-allocation"></a> **M-core-allocation** (0.11.0) needs: (none) — core allocation and heap primitives.
- exit: pass core allocator tests (`cargo test -p mnemosyne-memory-core --locked --all-features` passes).
- exit: pass local allocator and arena tests (`cargo test -p mnemosyne-local --locked --all-features` and `cargo test -p mnemosyne-arena --locked --all-features` pass).
- exit: pass facade allocation tests (`cargo test -p mnemosyne-memory --locked --all-features` passes).
- standing: no open correctness item.
<a id="M-policies-backends"></a> **M-policies-backends** (0.12.0) needs: M-core-allocation — policies, backends, and support crates.
- exit: pass backend and heap tests (`cargo test -p mnemosyne-backend --locked --all-features` and `cargo test -p mnemosyne-heap --locked --all-features` pass).
- exit: pass hardened/profile tests (`cargo test -p mnemosyne-hardened --locked --all-features` and `cargo test -p mnemosyne-prof --locked --all-features` pass).
- standing: no open correctness item.
<a id="M-1.0"></a> **M-1.0** (1.0.0) needs: M-policies-backends — stable allocator release (authority-gated).
- standing: `cargo fmt --all -- --check`, `cargo clippy --workspace --locked --all-targets --all-features -- -D warnings`, `cargo test --workspace --locked --all-features`, `cargo test --workspace --locked --doc --all-features`, and `cargo doc --workspace --locked --no-deps --all-features` pass; SemVer and publish dry-run gates required for declared releases pass; README and rustdoc are current; no open correctness item.
