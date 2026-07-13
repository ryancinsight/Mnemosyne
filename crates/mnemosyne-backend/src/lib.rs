//! Low-level OS page allocation backend mapping interface.
//!
//! The crate is organized by concern so each leaf owns one backend
//! responsibility:
//!
//! - [`mapping`] owns the [`MemoryBackendWrapper`] struct shape and the
//!   single `impl MemoryBackend for MemoryBackendWrapper` block.
//!   `allocate` / `deallocate` bodies are inline here; `make_guard`,
//!   `page_reset`, and `decommit` delegate into the per-concern helpers
//!   in [`guard`] and [`reset`] via `#[inline(always)]` static-dispatch
//!   calls.
//! - [`guard`] owns `do_make_guard` — the per-method helper for
//!   `PROT_NONE` / `PAGE_NOACCESS` guard-region installation — called
//!   by the `make_guard` entry in [`mapping`]'s impl block.
//! - [`reset`] owns `do_page_reset` and `do_decommit` — the per-method
//!   helpers for content-discard and commit-charge release — called by
//!   the `page_reset` and `decommit` entries in [`mapping`]'s impl block.
//! - [`recorders`] owns the telemetry counters, the
//!   [`BackendMemoryStats`] snapshot, and the per-concern unit tests
//!   for the `record_*` family.
//! - [`backends`] owns the per-OS / per-platform backend
//!   implementations (`UnixBackend`, `WindowsBackend`, the CUDA
//!   variants, and `WgpuStagingBackend`).
//!
//! Public re-exports at the crate root keep the canonical
//! `mnemosyne_backend::CudaUnifiedBackend`, `MemoryBackendWrapper`,
//! and `backend_memory_stats` paths while backend-specific helpers
//! live under [`backends`].

#![no_std]

pub mod backends;
pub mod guard;
pub mod mapping;
pub mod recorders;
pub mod reset;

pub use backends::DefaultBackend;
pub use backends::cuda::{
    CudaDeviceBackend, CudaHostPinnedBackend, CudaUnifiedBackend, is_cuda_available,
};
pub use backends::wgpu::{
    WgpuAllocateCallback, WgpuCallbackRegistrationError, WgpuCallbacks, WgpuDeallocateCallback,
    WgpuStagingBackend, register_wgpu_callbacks,
};
pub use mapping::MemoryBackendWrapper;
pub use recorders::{BackendMemoryStats, backend_memory_stats};
