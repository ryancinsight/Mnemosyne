# ADR 0011: Provide a portable WebAssembly memory backend

Status: Accepted

Date: 2026-09-06

Board item: [MN-WASM-2026-09-06](../../backlog.md#mn-wasm-2026-09-06)

## Context

The default memory backend selected Unix or Windows mapping primitives. That
left the core segment initialization and allocator fallback literals unable to
compile for 32-bit WebAssembly, even though the allocator's page contract is
representable with the host global allocator. The WASM target has no portable
OS mapping, protection, reset, or decommit operation.

## Decision

Select a dedicated `WasmBackend` for `target_arch = "wasm32"`. It allocates
and deallocates page-aligned blocks through `alloc::alloc` and reports the
unsupported page operations through the `MemoryBackend` capability constants.
Derive the segment key mask from `usize::MAX` so the same repeated bit pattern
is valid at every pointer width. Keep the native CUDA loader and host defaults
behind their existing target-family gates.

The backend is a complete allocation implementation, not a feature-gated
placeholder. Callers retain the `MemoryBackend` contract: allocation sizes are
non-zero and page-aligned, and deallocation receives the original pointer and
size.

## Alternatives rejected

* Reusing the Unix backend would import host mapping assumptions into WASM and
  leave the target without a valid implementation.
* Keeping the overflowing literals and suppressing the compiler would mask a
  pointer-width defect.
* Returning null pointers or making allocation a no-op would violate the
  allocator contract and would be a mock.

## Verification

On the clean `fix/mnemosyne-wasm-backend` branch from `origin/main`,
`cargo check --target wasm32-unknown-unknown --offline`, warning-denied Clippy
for native and `wasm32-unknown-unknown` all-target builds, and
`cargo nextest run --offline` (369/369) pass. The canary and stack-interner
fixtures use pointer-width-safe synthetic values so the same value contracts
are exercised on wasm32. The shared main checkout still carries unrelated peer
WIP; this branch does not claim to verify those uncommitted edits. The remaining
browser execution trace belongs to the Moirai browser reactor and Metis
integration items; this ADR only establishes the portable memory substrate.
