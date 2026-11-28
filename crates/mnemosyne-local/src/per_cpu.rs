//! Lock-free per-CPU L1 block caching.
//!
//! Stores block pointers in a flat atomic array behind the process-global
//! `PER_CPU_CACHE` handle, making it 100% memory-safe and UAF-free without
//! dereferencing block payload memory. The table is allocated only when the
//! cache is explicitly used.

mod ops;
mod state;
#[cfg(test)]
mod tests;
mod types;

pub use ops::{try_alloc_cpu, try_free_cpu};
pub use state::{
    PER_CPU_CACHE, PER_CPU_CACHE_ENABLED, current_cpu_id, disable_cpu_cache, enable_cpu_cache,
    get_current_cpu_id, refresh_current_cpu_id,
};
pub use types::{CpuCacheSlot, PerCpuCache, PerCpuCacheHandle};
