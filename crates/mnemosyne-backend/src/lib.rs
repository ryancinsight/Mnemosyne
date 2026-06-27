//! Low-level OS page allocation backend mapping interface.
//!
//! The crate is organized by concern so each leaf is one Rust
//! feature/concern with zero behavior change and zero benchmark risk:
//!
//! - [`mapping`] owns the [`MemoryBackendWrapper`] struct shape and the
//!   `allocate` / `deallocate` impl block (page-aligned mapping
//!   creation and release), forwarding confirmed OS releases to the
//!   [`recorders`] layer.
//! - [`guard`] owns the `make_guard` impl block
//!   (`PROT_NONE` / `PAGE_NOACCESS` install).
//! - [`reset`] owns the `page_reset` and `decommit` impl blocks
//!   (content-discard and commit-charge release).
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

pub use backends::cuda::{
    is_cuda_available, CudaDeviceBackend, CudaHostPinnedBackend, CudaUnifiedBackend,
};
pub use backends::wgpu::WgpuStagingBackend;
pub use backends::DefaultBackend;
pub use mapping::MemoryBackendWrapper;
pub use recorders::{backend_memory_stats, BackendMemoryStats};

use core::ffi::c_void;
use core::sync::atomic::AtomicPtr;

/// Global static callback for hooking a third-party allocator's
/// allocate path (typically wgpu's staging allocation hook) into
/// Mnemosyne. [`crate::backends::wgpu::WgpuStagingBackend`] reads
/// this pointer on every `allocate`; consumers (e.g.
/// `hephaestus-wgpu`) register their own function pointer at startup.
pub static WGPU_ALLOCATE_CALLBACK: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

/// Global static callback for hooking a third-party allocator's
/// deallocate path into Mnemosyne. Mirror of
/// [`WGPU_ALLOCATE_CALLBACK`] for the release side.
pub static WGPU_DEALLOCATE_CALLBACK: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
