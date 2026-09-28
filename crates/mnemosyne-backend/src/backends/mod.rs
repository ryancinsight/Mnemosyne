//! Per-OS and platform-specific memory backends.
//!
//! Each leaf owns one backend's `MemoryBackend` impl and any
//! per-platform helpers it needs. Each leaf names its backend
//! `DefaultBackend`; exactly one leaf is compiled per target, so the
//! re-export below is the platform's default backend with no alias layer.
//! The `HasSegmentPool` impls for each backend live in `mnemosyne-arena`'s
//! segment-pool module.

#[cfg(target_family = "windows")]
mod windows;
#[cfg(target_family = "windows")]
pub use self::windows::DefaultBackend;

#[cfg(target_family = "unix")]
mod unix;
#[cfg(target_family = "unix")]
pub use self::unix::DefaultBackend;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use self::wasm::DefaultBackend;

pub mod cuda;
