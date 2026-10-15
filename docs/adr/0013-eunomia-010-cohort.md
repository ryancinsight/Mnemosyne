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

Use the canonical Git URL and a Cargo-managed precise lock to the reviewed
Eunomia 0.10 candidate. Final acceptance requires the coordinated merged and
published source. No Rust implementation, pointer/layout behavior, fixture,
compatibility shim, test or timeout change is part of this increment.

## Verification and current blocker

Run the existing ordinary native, lint, docs, package and API gates. Those
checks do not establish Miri coverage. Root Cargo manifest/lock changes set
code=true in the existing CI classifier, selecting Miri; the required CI
aggregate includes that job. Miri is explicitly deferred and unverified.
Do not create a PR/merge that starts it or change the classifier/gate to obtain
a passing status. Keep this provider increment blocked until the task owner
resolves that scope conflict. Existing held safety/performance PRs and their
reserved ADR 0012 remain untouched; no safety investigation is resumed.
