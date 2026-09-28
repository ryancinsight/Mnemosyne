# Guard Pages

A guard page is a protected memory region installed immediately beside live
allocator data: an out-of-bounds write that reaches it faults immediately
instead of silently corrupting an adjacent segment header or a neighboring
mapping. It is independent of the [`SecurePolicy`/`HardenedPolicy` mitigation
flags](hardened_secure.md) — those specialize the allocator's *data-plane*
behavior (zeroing, poisoning, free-list encryption) at the type level; guard
pages are a *layout*-level defense installed by the arena on the raw OS
mapping, and apply the same way regardless of which policy a given allocator
instance uses.

## The backend seam

`MemoryBackend::make_guard(ptr, size) -> bool` marks a page-aligned range
inaccessible:

```rust,ignore
unsafe fn make_guard(ptr: *mut u8, size: usize) -> bool;
```

The default implementation returns `false`, so a backend with no equivalent
operation opts out without breaking the trait. Two backends implement it for
real:

- **Unix** — `mprotect(ptr, size, PROT_NONE)`. Every Unix target Mnemosyne
  supports implements `mprotect`, so `SUPPORTS_MAKE_GUARD = true`
  unconditionally.
- **Windows** — `VirtualProtect(ptr, size, PAGE_NOACCESS, &mut old_protect)`.
  `SUPPORTS_MAKE_GUARD = !cfg!(miri)`: Miri does not implement
  `VirtualProtect`, so guard installation reports failure under the
  interpreter rather than being exercised.

The WASM backend and every CUDA backend (unified, device, HBM, GDDR,
host-pinned) report `SUPPORTS_MAKE_GUARD = false` — the device-tier CUDA
backends forward the constant from `CudaDeviceBackend`, which does not
override the trait's `false` default, so guard pages never install on CUDA
memory. `make_guard` rejects a null pointer or zero size before reaching
the OS call.

A confirmed install is recorded by `mnemosyne-backend`'s telemetry layer
without touching `current_mapped_bytes` — the mapping stays reserved, only
its protection bits change — and surfaces as `guard_install_calls` /
`guard_install_bytes` on `BackendMemoryStats`, re-exported through
`mnemosyne::MemoryStats`.

## The arena consumer

The backend seam alone installs nothing; `mnemosyne-arena`'s `allocate_segment`
is the consumer, gated behind two opt-in Cargo features (mirrored on the
`mnemosyne` facade so an application crate does not need to depend on
`mnemosyne-arena` directly):

| Feature | Guard location | Catches |
| --- | --- | --- |
| `segment-tail-guards` | `aligned_addr + SEGMENT_SIZE`, inside the `SEGMENT_MAPPING_SIZE - SEGMENT_SIZE` alignment slack the arena already reserves | forward (over-the-end) writes past the last page |
| `segment-header-guards` | end of Page 0, at `aligned_addr + PAGE_SIZE - SEGMENT_HEADER_GUARD_SIZE` | backward (underflow) writes into the segment metadata header |

Both guard sizes are one page: `SEGMENT_TAIL_GUARD_SIZE` and
`SEGMENT_HEADER_GUARD_SIZE` are each `4096` bytes, fixed at compile time
(`const _: () = assert!(SEGMENT_TAIL_GUARD_SIZE.is_power_of_two());` and
matching checks bound each to `SEGMENT_ALIGN` / `PAGE_SIZE`).

Each install happens once per fresh OS-backed segment — the cold path,
never per allocation — and only when its feature is enabled **and** the
active backend's `B::SUPPORTS_MAKE_GUARD` is `true`. Neither feature is on
by default, so the benchmarked allocator paths keep zero guard-install
overhead unless a build opts in.

## Best-effort, not guaranteed

Guard installation is deliberately best-effort. A backend that declined
(`SUPPORTS_MAKE_GUARD == false`) or a kernel that rejects the request skips
silently, leaving the alignment slack accessible rather than failing the
allocation — for example, on macOS/aarch64 the OS page size is 16 KiB, so a
4 KiB-granularity `mprotect` request there is not guaranteed to hold the
way it does on a 4 KiB-page target. The install is never asserted to
succeed; `guard_install_calls` is the observable that tells you how many
installs a given run actually confirmed, so verifying guard coverage in a
specific deployment means reading the counter, not trusting the feature
flag alone.

## Segments recycled from the pool

A segment popped from the retained free-segment pool skips fresh
initialization entirely (NUMA binding, guard installation, slack decommit)
— it keeps whatever guards were installed when it was first mapped from the
OS. Guard placement is therefore a property of a segment's *first*
allocation in its mapping's lifetime, not something re-applied on every
reuse.
