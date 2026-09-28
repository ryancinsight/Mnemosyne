//! Highly optimized, monomorphized Thread Local Storage (TLS) provider implementations.
//!
//! # Module Organization
//!
//! | Sub-module | Contents |
//! |---|---|
//! | [`traits`] | `TlsSlotAccess<B>`, `TlsProvider<B>` sealed traits |
//! | [`stable`] | `StandardTls`, `CachedCellTls` (stable-channel `thread_local!`) |
//! | [`native`] | `NativeOsTls` (`TlsGetValue`/`pthread_getspecific`), `AsmTls` (TEB inline ASM) |
//! | [`nightly`] | `NightlyTls` (`#[thread_local]` nightly path) |
//!
//! The raw platform-native TLS FFI (OS key init, get, set, and the Windows
//! x86-64 TEB ASM helpers) is not declared here: it lives once in
//! [`mnemosyne_core::os_tls`] so this crate and `mnemosyne-prof` share one
//! `unsafe` implementation.
//!
//! # Selection Strategy
//!
//! Select the fastest provider available for the target:
//! - Windows x86_64, nightly: `NightlyTls`
//! - Windows x86_64, stable: `AsmTls` (TEB inline ASM)
//! - Windows x86_64, portable: `NativeOsTls` or `CachedCellTls`
//! - POSIX, all: `CachedCellTls` or `NativeOsTls`
//! - Generic fallback: `StandardTls`

pub mod native;
pub mod nightly;
pub mod stable;
pub mod traits;

pub use native::{AsmTls, NativeOsTls};
pub use nightly::NightlyTls;
pub use stable::{CachedCellTls, StandardTls};
pub use traits::{TlsProvider, TlsSlotAccess};
