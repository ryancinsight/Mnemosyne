# mnemosyne-backend

Memory backend implementations for the
[Mnemosyne](https://github.com/ryancinsight/mnemosyne) allocator: the layer that
turns `MemoryBackend` calls into OS virtual-memory operations.

```toml
[dependencies]
mnemosyne-backend = "0.5"
```

## Layout, by concern

- `mapping` — the `MemoryBackendWrapper` struct and the single central
  `impl MemoryBackend for MemoryBackendWrapper` block (trait coherence requires
  one file). `allocate` and `deallocate` live inline; `make_guard`,
  `page_reset`, and `decommit` delegate to the helpers below via
  `#[inline(always)]` static dispatch.
- `guard` — `PROT_NONE` (`mprotect`) / `PAGE_NOACCESS` (`VirtualProtect`) guard
  installation, used for segment-tail out-of-bounds trapping.
- `reset` — `page_reset` (`MADV_DONTNEED` on Linux, `MADV_FREE` on
  macOS/FreeBSD, `MEM_RESET` on Windows) and `decommit`. These drop physical
  backing while keeping the virtual mapping committed.
- `recorders` — telemetry counter statics and the `BackendMemoryStats` snapshot,
  reachable publicly through `backend_memory_stats()`.
- `backends` — `UnixBackend`, `WindowsBackend`, the CUDA backends
  (`CudaUnifiedBackend`, `CudaDeviceBackend`, tier-keyed `CudaHbmBackend` and
  `CudaGddrBackend`, `CudaHostPinnedBackend`), and `DefaultBackend`, which
  selects the OS-conditional backing at compile time.

## Release accounting

`MemoryBackend::deallocate` returns a release-success boolean and is
`#[must_use]`. `current_mapped_bytes` is decremented only on confirmed OS
release; a failed `munmap`/`VirtualFree` routes through `record_unmap_failure`
so the counter cannot under-count still-mapped bytes. `page_reset` and
`make_guard` never decrement it — the mapping stays owned, only the resident set
drops.

On Linux, segment-sized mappings receive a `madvise(MADV_HUGEPAGE)` hint. The
hint is advisory; failure never invalidates the mapping.

Licensed under MIT OR Apache-2.0.
