# ADR 0013: Adopt the Eunomia 0.10 datatype provider

- Status: Accepted
- Date: 2026-10-15
- Item: MN-EUNOMIA-010-COHORT [major]

## Contract and versioning

The datatype provider's CastFrom/CastTo retirement requires Eunomia 0.10.
The arena's optional typed-buffer marker bounds expose that provider identity.
Local allocators and branded heaps expose the arena HasSegmentPool bound in
public generic signatures; the facade reexports those surfaces. Advance arena,
local and heap to 0.7.0, and the facade to 0.10.0, with matching internal edges.
Decay and the C shim keep their public interfaces and use patch 0.3.3 for their
updated private dependencies. Preserve the C ABI and all other package versions.

Use the canonical Git URL and a Cargo-managed precise lock to the published
Eunomia 0.10.0 source a231e54dd3311bc916e78c91d994b00d651c1646. The Cargo lock also
selects current [Themis](https://github.com/ryancinsight/themis/commit/63c544108fc0e8408bf2096ddaf8e99dfa49f90c)
and [Melinoe](https://github.com/ryancinsight/melinoe/commit/681c550efa385ade0dd1e4697451e80ac26b7ab5) without changing their version floors; all native suites are rerun on that graph.
Default allocation/pool/registry behavior, fixtures and timeouts stay unchanged.
The independent optional-guard correction below changes only protected extents. Gate the scratch-implementation macro with the same non-bytemuck
predicate as its invocation, fixing an unused-macro diagnostic in the existing
all-feature configuration. Render the Windows-only TLS function name as code
so warning-denied rustdoc also works on macOS. No compatibility shim is added.

The ordinary native allocation-counting isolation test also fails on unchanged
main: platform Barrier waits allocate lazily inside the caller's zero-allocation
window. Replace only that test's coordination with allocation-free atomic
acquire/release signals. Keep the 1024-byte worker allocation, every assertion,
the existing Miri configuration and the native time budget unchanged. This
repairs the test's measured boundary without changing allocator behavior or
resuming the held safety/performance work.

The corrected default native suite passes all 481 cases with zero skips.
The optional-guard failure also reproduces on unchanged main. Its independent
repair queries native OS page size with sysconf/GetSystemInfo, validates
power-of-two geometry, and requests the larger of a native page and the public
4096-byte minimum. Both sizes are powers of two, so the maximum is a multiple
of each. Header ranges start beyond all live Segment metadata; tail ranges
must fit the existing mapping, and slack decommit starts at the complete guard
end. Unsupported geometry leaves the best-effort guard inactive. No backend
trait or mapping/registration logic is changed.

On 16-KiB macOS pages, tail/header/both now confirm 1/16384,1/16384 and 2/32768
calls/bytes. The original telemetry/mapping/release assertions are unchanged;
address fixtures also check page preconditions, and slack decommit requires the
exact unprotected remainder. Default 481, individual guards 149 each and all-feature
arena 151 cases pass with zero skips. Three geometry cases cover 4/16/64-KiB
pages, invalid geometry and metadata exclusion. Windows/Linux cross checks
compile; native Windows runtime remains unverified. Final Linux guard-matrix
and required Miri evidence are recorded below.
Strict all-target/all-feature Clippy,
warning-denied docs, 26 doctests (one existing ignore) and all six standard
package verifications pass. Locked-source API classification accepts the four
minor boundaries (253 rules skipped by the tool for each major-compatible
release class) and compares 196 rules for each dependency-only patch. This
does not establish backward compatibility across the minor boundaries.

## Final acceptance and immutable receipts

[Reviewed head](https://github.com/ryancinsight/Mnemosyne/commit/f36febc82edfc1a1fbc7df92b6847fc894f5b859)
and [published merge](https://github.com/ryancinsight/Mnemosyne/commit/b7e0e1e37130b0a317859b168cca16b5172d08d8)
share an [identical tree](https://github.com/ryancinsight/Mnemosyne/tree/9c05a3011ca7d3a15e5e1f21d60bd96a74c5fc83). All required exact-head
checks pass, including actual Miri, Loom, ThreadSanitizer, Linux native guard
matrix and CI aggregate. The blocking [release validation](https://github.com/ryancinsight/Mnemosyne/actions/runs/38749847652)
builds and compares all ten selected library baselines and verifies the arena
archive. The existing caller excludes the unchanged build-only utility; no
workflow, baseline fallback, release-type override, coverage or deadline change
supplies acceptance. All six clean final-head standard packages also verify.

The normal guarded merge triggers the existing [publisher](https://github.com/ryancinsight/Mnemosyne/actions/runs/38752721843),
which succeeds. All six downloaded archives independently match their registry
SHA-256 and `.cargo_vcs_info.json` source b7e0e1e; all are present and not yanked.

| Crate | Version | Registry SHA-256 |
| --- | --- | --- |
| `mnemosyne-arena` | 0.7.0 | `581052478bcdb6749e17a3648ce938814468a56a242feec22363614c3c79b414` |
| `mnemosyne-local` | 0.7.0 | `9e2e5729346946d80bf14523d707e3dc3f7e347b09895d6cfb98ba0174702406` |
| `mnemosyne-heap` | 0.7.0 | `baaf13fcadc5d699ee71d5bc157e45c739cf2ed7c713a120667dc803ab1e31dc` |
| `mnemosyne-memory` | 0.10.0 | `4b442affb7cf56fee5446facc7dcdb15ec7a14a7038f0646727ec0736479ff74` |
| `mnemosyne-decay` | 0.3.3 | `5123cfd9231741d692b3860f0498577962a81aac8b33018a431587ab5c6e9b2c` |
| `mnemosyne-c-shim` | 0.3.3 | `c9db92dd714df63c275ca9e1359b4d14c9d3d069d95d2ec6b6395c97add71489` |

The user explicitly authorizes required Miri and publication after all gates
pass. Required-pipeline Miri establishes only its unchanged declared scope;
held free-path/sampler safety and performance work and reserved ADR 0012 remain
untouched. Their existing backlog items stay open, and native Windows runtime
is unverified. This receipt-only closure changes no Rust, version, lock, workflow
or native deadline and requests no additional package version or deployment.
