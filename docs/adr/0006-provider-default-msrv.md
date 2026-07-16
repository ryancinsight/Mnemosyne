# ADR 0006: Provider-default MSRV alignment

Status: accepted

## Context

Mnemosyne's direct Eunomia dependency follows the provider default branch.
That source declares Rust 1.95, while published Mnemosyne manifests still
declared Rust 1.87. Keeping the obsolete lower declaration would advertise a
toolchain that Cargo cannot resolve. Retaining a revision-qualified Eunomia
dependency would fork the Atlas source graph in downstream Moirai, Leto, and
Apollo consumers.

## Options

- Keep the Eunomia revision pin and the 1.87 declaration. This preserves a
  false lower MSRV and blocks source convergence.
- Follow the provider default source while leaving 1.87 declared. This lets
  Cargo reject affected consumers after selection and is an invalid contract.
- Follow the provider default source, declare Rust 1.95, and advance every
  published pre-1.0 Mnemosyne package version. This makes the compiler and
  package metadata agree.

## Decision

Select the third option. Mnemosyne follows Eunomia and Melinoe provider default
branches, removes source quarantine, declares Rust 1.95 for every workspace
crate, and advances each published pre-1.0 package version. `Cargo.lock` is the
sole reproducibility pin.

## Consequences

Consumers must update to Rust 1.95 before adopting the new Mnemosyne graph and
must refresh their package-version requirements where they constrain a
Mnemosyne crate. The change does not alter allocator algorithms or introduce a
compatibility path. Its acceptance evidence is Rust 1.95 compilation plus the
existing value-semantic allocator and provider-identity gates.
