//! C ABI shim exposing Mnemosyne through the standard `malloc` family.
//!
//! The functions here mirror the C standard / POSIX allocator surface so
//! Mnemosyne can be used from C/C++ code or interposed via `LD_PRELOAD`
//! (Unix) / DLL injection (Windows). They route to the same thread-local
//! allocator the Rust `#[global_allocator]` path uses, through
//! `mnemosyne_local::{thread_alloc, thread_free, usable_size}` under the
//! standard policy and OS-mapping backend.
//!
//! ## C vs. Rust copy-length semantics
//!
//! The Rust `GlobalAlloc::realloc` path copies only `layout.size()` bytes
//! because the Rust contract tracks the originally-requested size. C has
//! no such tracking: `realloc` must preserve the lesser of the *old usable
//! size* and the new size, because a C caller may legitimately have
//! written the entire usable region returned by `malloc`. The shim's
//! `realloc` therefore copies `min(usable_size(old), new_size)` — the
//! correct and safe choice for C semantics, and distinct from the Rust
//! path on purpose.

#![no_std]
#![deny(missing_docs)]

#[cfg(test)]
use core::ffi::c_void;

mod aligned_api;
mod core_api;
mod prof_api;

pub use aligned_api::{aligned_alloc, posix_memalign};
pub use core_api::{calloc, free, malloc, malloc_usable_size, realloc};
pub use prof_api::{
    mnemosyne_disable_leak_detector, mnemosyne_disable_profiling, mnemosyne_dump_leaks,
    mnemosyne_dump_profile, mnemosyne_enable_leak_detector, mnemosyne_enable_profiling,
    mnemosyne_is_leak_detector_enabled, mnemosyne_is_profiling_enabled,
    mnemosyne_register_alloc_hook, mnemosyne_register_free_hook,
    mnemosyne_reset_profiler_for_testing,
};

/// Minimum alignment the C `malloc`/`calloc`/`realloc` family must
/// guarantee. The C standard requires the result to be suitably aligned
/// for any object with a fundamental alignment requirement; on every
/// supported 64-bit target that is `max_align_t == 16`.
pub(crate) const MALLOC_ALIGN: usize = 16;

/// POSIX `EINVAL`, returned by `posix_memalign` for an invalid alignment.
pub(crate) const EINVAL: i32 = 22;
/// POSIX `ENOMEM`, returned by `posix_memalign` when the allocation fails.
pub(crate) const ENOMEM: i32 = 12;

#[cfg(test)]
mod tests;
