# Position in the Stack

Mnemosyne is the **memory allocator of the Atlas stack**: it owns how memory
is mapped, cached, reused, and secured, and every higher layer that
allocates runs on top of it.  It is a *provider* crate — a leaf that other
providers and integrators consume — rather than an integrator that glues
other providers together.

## Consumers

Anything that runs inside Atlas and allocates is, transitively, a Mnemosyne
consumer.  The direct consumers are the scientific and imaging stacks that
need predictable, low-contention allocation:

- **kwavers** and **apollo** use the scratch-pool pattern for transient
  FFT-window and solver-residual buffers (see the
  [Scratch Pool example](examples/scratch_pool.md)).
- Providers that run heavy numeric workloads benefit from the per-thread
  cache and the size-class table with no code changes — they just link the
  allocator.

## Provider relationships

Mnemosyne depends on three sibling providers, each contributing exactly one
concern:

| Provider | Contribution | Where |
| --- | --- | --- |
| **themis** | Topology and placement vocabulary | `MemoryTier`/`PlacementHint` re-exported by `mnemosyne-heap`; used by the NUMA and tiered paths |
| **eunomia** | Numeric element types | Optional feature (`mnemosyne/eunomia`) for the arena's element handling |
| **melinoe** | Branding lifetimes for ownership | `InvariantLifetime`, `ThreadLocalToken`, and `thread_local_scope` underpin the branded heap |

The branding seam is the allocator's answer to ownership: `BrandedBlock`,
`BrandedBox`, and `BrandedVec` tie each block to a scope-local
`InvariantLifetime` so the *type system*, not runtime checks, prevents a
block outliving the heap or scope that owns it.  `scope`/`scope_tiered`
provide the scoped entry points.  This is the same melinoe branding
vocabulary used across the Atlas stack, so an object branded by one provider
carries the same lifetime discipline into Mnemosyne.

## Standalone consumption

Mnemosyne is also usable **outside** the Atlas workspace.  The workspace
dependency declarations keep git sources with published-version requirements,
so a standalone consumer resolves the provider graph (themis, eunomia,
melinoe) without Atlas-local sibling paths.  The `mnemosyne` and
`mnemosyne-core` package identities are published to crates.io as
**`mnemosyne-memory`** and **`mnemosyne-memory-core`** (ADR 0007) because
the plain names are occupied by unrelated projects; the Rust library names
are unchanged.  A GitHub Release workflow validates crate identity and
package contents before publishing through crates.io Trusted Publishing.

## Boundary in the Atlas taxonomy

- Mnemosyne is the **memory** authority.  It does not implement numerics
  (eunomia), topology (themis), or branding policy (melinoe) — it consumes
  them.
- It does not make placement *decisions* for applications; it executes them.
  `PlacementHint` comes from themis and is honored by the tiered heap, but
  the caller chooses.
- Its policy flags are compile-time types, so consumer code can rely on the
  guarantees (zeroing, poisoning, encryption) being real at the machine-code
  level, not runtime best-effort.
