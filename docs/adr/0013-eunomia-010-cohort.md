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
compile; native Windows and final Linux/Miri results remain separate evidence.
Strict all-target/all-feature Clippy,
warning-denied docs, 26 doctests (one existing ignore) and all six standard
package verifications pass. Locked-source API classification accepts the four
minor boundaries (253 rules skipped by the tool for each major-compatible
release class) and compares 196 rules for each dependency-only patch. This
does not establish backward compatibility across the minor boundaries.

## Verification and current blocker

Run the existing ordinary native, lint, docs, package and API gates. Those
checks do not establish Miri coverage. Root Cargo manifest/lock changes set
code=true in the existing CI classifier, selecting Miri; the required CI
aggregate includes that job. The user now authorizes these existing required
Miri checks; actual exact-head results remain pending. Do not change the
classifier, gate, filters or timeouts to obtain a passing status. This resumed
turn now authorizes publication after all applicable exact-head gates and
reviews pass; unresolved native guard acceptance still blocks release. Existing held safety/performance
PRs and reserved ADR 0012 remain untouched; their implementation work is not
resumed by running the current required CI.
