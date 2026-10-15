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
Eunomia 0.10.0 source a231e54dd3311bc916e78c91d994b00d651c1646.
Production behavior, pointer/layout behavior, fixtures and timeouts stay
unchanged. Gate the scratch-implementation macro with the same non-bytemuck
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
The optional Eunomia/both-guard suite runs 148 cases: 147 pass and the guard
telemetry case fails on macOS. That exact case also fails on unchanged main.
Keep this separate prerequisite visible; do not alter its assertions or the
guard implementation in this increment. Strict all-target/all-feature Clippy,
warning-denied docs, 26 doctests (one existing ignore) and all six standard
package verifications pass. Locked-source API classification accepts the four
minor boundaries (253 rules skipped by the tool for each major-compatible
release class) and compares 196 rules for each dependency-only patch. This
does not establish backward compatibility across the minor boundaries.

## Verification and current blocker

Run the existing ordinary native, lint, docs, package and API gates. Those
checks do not establish Miri coverage. Root Cargo manifest/lock changes set
code=true in the existing CI classifier, selecting Miri; the required CI
aggregate includes that job. Miri is explicitly deferred and unverified.
Do not create a PR/merge that starts it or change the classifier/gate to obtain
a passing status. Keep this provider increment blocked until the task owner
resolves that scope conflict. Existing held safety/performance PRs and their
reserved ADR 0012 remain untouched; no safety investigation is resumed.
