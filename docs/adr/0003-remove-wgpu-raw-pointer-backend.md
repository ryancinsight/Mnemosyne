# ADR 0003: Remove the WGPU raw-pointer backend

- Status: accepted
- Date: 2026-07-13
- Change class: major

## Context

`WgpuStagingBackend` adapts mapped WGPU buffers to `MemoryBackend`, whose
allocation contract returns a generally writable raw pointer. WGPU 30 no
longer exposes a mutable pointer or `DerefMut` from `BufferViewMut`. Its mapped
write view supports only explicit byte-slice writes because the underlying
memory may use write-combining semantics. A pointer satisfying
`MemoryBackend` would therefore assert capabilities the provider does not
grant.

## Decision

Delete `WgpuStagingBackend`, its process-global callback registry, backend
selector implementations, and public re-exports. WGPU buffer creation,
mapping, byte transfer, and lifetime ownership remain in Hephaestus, the crate
that owns the WGPU provider. Mnemosyne continues to own CPU and CUDA allocation
backends whose memory contracts satisfy `MemoryBackend`.

## Rejected alternatives

- Reinterpreting `BufferViewMut` to recover a pointer relies on private layout
  and violates the provider contract.
- A callback that copies through temporary host memory is not a memory backend
  and would create a hidden copy and false `HostPinned` semantics.
- Retaining the obsolete API as a compatibility wrapper preserves an invalid
  contract and duplicates staging ownership.

## Consequences

This removes public items from pre-1.0 `mnemosyne-backend` and `mnemosyne`, so
both crates receive minor-version bumps and the changelog records the breaking
migration. Consumers replace allocator-shaped staging with provider-native
WGPU buffer operations.

## Verification

The evidence is compile-time API enforcement plus value-semantic workspace
tests. Hephaestus separately verifies WGPU 30 upload, allocation, and readback
contracts through its provider package gates.
