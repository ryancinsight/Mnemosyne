//! Process-global per-CPU cache state: enable/disable toggles, the
//! backend-keying-invariant static, and the cached-current-CPU-id helpers.

use super::types::{PerCpuCache, PerCpuCacheHandle};
use core::sync::atomic::{AtomicBool, Ordering};

pub(super) const MAX_CACHED_BLOCKS: usize = 8;
pub(super) const NUM_CPU_SLOTS: usize = 256;

/// Whether the per-CPU block cache is consulted on the alloc/free fast
/// path.
///
/// Disabled under `cfg(test)` so tests observe the thread-local path
/// deterministically: the per-CPU cache can otherwise satisfy a request
/// from another CPU's slot and make occupancy assertions flaky.
#[cfg(test)]
pub static PER_CPU_CACHE_ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether the per-CPU block cache is consulted on the alloc/free fast
/// path.
#[cfg(not(test))]
pub static PER_CPU_CACHE_ENABLED: AtomicBool = AtomicBool::new(true);

/// The global per-CPU cache instance.
///
/// # Backend-keying invariant (latent hazard)
///
/// This is a single process-global array, **not** keyed by memory backend. The
/// slots store raw block addresses, so a block freed here under one backend and
/// re-handed out under another would cross backend ownership. `try_alloc_cpu` /
/// `try_free_cpu` only touch it when `B::ENABLE_CPU_CACHE` is `true`, and today
/// no backend sets that constant to `true` (the `MemoryBackend` trait default
/// is `false`, and neither the Unix nor Windows `DefaultBackend` overrides it),
/// so exactly one backend — none, currently — can ever populate the cache and
/// no mixing is possible.
///
/// The correctness of the shared static therefore rests on the invariant that
/// **at most one backend enables the CPU cache per process**. Keying the cache
/// per backend (an associated-type or generic buffer on `ComputeBackend`) is the
/// robust fix but is a `[minor]`-class change (new trait surface); until then this
/// invariant is documented rather than type-enforced. Any future backend that
/// sets `ENABLE_CPU_CACHE = true` alongside another such backend must first
/// introduce that per-backend keying.
pub static PER_CPU_CACHE: PerCpuCacheHandle = PerCpuCacheHandle::new();

const _: () =
    assert!(core::mem::size_of::<PerCpuCacheHandle>() < core::mem::size_of::<PerCpuCache>());

pub(super) static DISABLE_CPU_CACHE: AtomicBool = AtomicBool::new(false);

/// Dynamically disables the per-CPU cache.
pub fn disable_cpu_cache() {
    DISABLE_CPU_CACHE.store(true, Ordering::Relaxed);
}

/// Dynamically enables the per-CPU cache.
pub fn enable_cpu_cache() {
    DISABLE_CPU_CACHE.store(false, Ordering::Relaxed);
}

/// Returns the current CPU ID.
#[inline]
pub fn current_cpu_id() -> usize {
    themis::current_processor().map_or(0, |cpu| cpu as usize) % NUM_CPU_SLOTS
}

melinoe::thread_cached! {
    /// Per-thread cached CPU id (melinoe `thread_cached!` SSOT; replaces the
    /// crate-local TLS pair and its `usize::MAX` sentinel — uninitialized is
    /// now a real `Option` state).
    mod cached_cpu_id: usize;
}

/// Returns the cached CPU ID, or queries the OS and caches it if uninitialized.
#[inline(always)]
pub fn get_current_cpu_id() -> usize {
    cached_cpu_id::get_or_init(current_cpu_id)
}

/// Force-refreshes the cached CPU ID from the OS.
#[inline(always)]
pub fn refresh_current_cpu_id() -> usize {
    let actual = current_cpu_id();
    cached_cpu_id::set(actual);
    actual
}
