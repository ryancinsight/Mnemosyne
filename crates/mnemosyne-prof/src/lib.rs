//! Heap profiling runtime for the Mnemosyne allocator: user alloc/free trace
//! hooks, a Poisson heap sampler, and an every-allocation leak detector, all
//! reached through the `on_alloc`/`on_free` entry points the allocator crates
//! call on every allocation and deallocation.

#![cfg_attr(nightly_tls_active, feature(thread_local))]
#![deny(missing_docs)]

mod control;
mod hooks;
mod sampler;
#[cfg(test)]
mod tests;
mod tls;

pub use control::{
    disable_leak_detector, disable_profiling, enable_leak_detector, enable_profiling, is_active,
    is_leak_detector_enabled, is_profiling_enabled, register_alloc_hook, register_free_hook,
    reset_profiler_for_testing,
};
pub use hooks::{on_alloc, on_free};
pub use sampler::{Sample, StackId, dump_leaks, dump_profile};

pub(crate) use tls::{enter_hook, exit_hook, sample_debit};
// `get_profiler_state` exists only on the non-nightly TLS backend (the nightly
// `#[thread_local]` path reads `THREAD_STATE` directly), so its import must
// carry the same cfg as its definition — importing it unconditionally is an
// E0432 whenever `nightly_tls_active` fires. Its sole use site
// (`sampler.rs`) is already `#[cfg(not(nightly_tls_active))]`.
#[cfg(not(nightly_tls_active))]
pub(crate) use tls::get_profiler_state;
#[cfg(nightly_tls_active)]
pub(crate) use tls::{get_bytes_until_sample, set_bytes_until_sample};
