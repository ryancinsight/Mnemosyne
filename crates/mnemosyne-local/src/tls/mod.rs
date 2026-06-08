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
//! | [`os_helpers`] | Platform-native TLS key init, get, set functions and TEB ASM helpers |
//!
//! # Selection Strategy
//!
//! Select the fastest provider available for the target:
//! - Windows x86_64, nightly: `NightlyTls`
//! - Windows x86_64, stable: `AsmTls` (TEB inline ASM)
//! - Windows x86_64, portable: `NativeOsTls` or `CachedCellTls`
//! - POSIX, all: `CachedCellTls` or `NativeOsTls`
//! - Generic fallback: `StandardTls`

pub mod traits;
pub mod stable;
pub mod native;
pub mod nightly;
pub(crate) mod os_helpers;

pub use traits::{TlsSlotAccess, TlsProvider};
pub use stable::{StandardTls, CachedCellTls};
pub use native::{NativeOsTls, AsmTls};
pub use nightly::NightlyTls;
