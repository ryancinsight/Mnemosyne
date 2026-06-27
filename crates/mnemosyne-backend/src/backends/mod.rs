//! Per-OS and platform-specific memory backends.
//!
//! Each leaf owns one backend's `MemoryBackend` impl and any
//! per-platform helpers it needs. The `DefaultBackend` alias resolves
//! to the platform's default backend, gated by `target_family` so
//! cross-compiles cleanly fail-fast. The `HasSegmentPool` impls for
//! each backend live in `mnemosyne-arena`'s segment-pool module.

#[cfg(target_family = "windows")]
mod windows;
#[cfg(target_family = "windows")]
pub use self::windows::WindowsBackend as DefaultBackend;

#[cfg(target_family = "unix")]
mod unix;
#[cfg(target_family = "unix")]
pub use self::unix::UnixBackend as DefaultBackend;

pub mod cuda;
pub mod wgpu;
