//! Thread-local cache allocation and deallocation routing.

#![no_std]
#![deny(missing_docs)]
// The `nightly_tls` feature requests the ELF/PE `#[thread_local]` accessor.
// The unstable path is compiled only when the active compiler is nightly;
// stable builds, including `--all-features`, use the portable TLS provider.
#![cfg_attr(nightly_tls_active, feature(thread_local))]

extern crate std;

#[cfg(miri)]
pub mod miri_cleanup;

#[cfg(miri)]
pub use miri_cleanup::miri_cleanup_pools;

pub mod fast_path_cache;
pub mod local_alloc;
pub mod per_cpu;
pub mod tls;

// Phase 4 instrumentation probe. Opt-in via the `dealloc-probe`
// Cargo feature; production builds compile the module out and pay
// zero cost in `thread_free`.
#[cfg(feature = "dealloc-probe")]
pub mod dealloc_counters;

/// Per-size-class allocation telemetry.
///
/// Process-wide `alloc_count`, `dealloc_count`, and `alloc_bytes` per size
/// class. The fast path updates bounded per-thread counters and batches
/// relaxed atomic updates, avoiding a shared cache-line RMW per alloc/free.
/// Snapshots flush the calling thread; other threads may retain at most one
/// partial batch per direct-mapped counter until another operation or thread
/// teardown flushes it. Use [`bin_stats::bin_snapshot`] or
/// [`bin_stats::all_bin_snapshots`] to read snapshots for profiling and
/// fragmentation monitoring.
pub mod bin_stats;

mod alloc;
mod free;
mod free_helpers;
mod options;
mod realloc;
pub mod tls_slot;
mod usable_size;
mod validation;

#[cfg(test)]
mod tests;

pub use alloc::{thread_alloc, thread_alloc_layout};
pub use bin_stats::{
    BinSnapshot, all_bin_snapshots, alloc_distribution, bin_snapshot, flush_tls_stats,
    hottest_class, reset_bin_stats, reset_generation_count, summary_line, total_alloc_count,
    total_internal_fragmentation, total_live_bytes, total_requested_bytes,
};
pub use fast_path_cache::{
    FastPathCacheConfig, FastPathCacheManager, FastPathEfficiencyMetrics, SizeClassCache,
};
pub use free::{thread_free, thread_free_layout};
pub use local_alloc::{SizeClassOccupancy, ThreadAllocator, ThreadAllocatorStats};
pub use realloc::thread_realloc;
pub use tls_slot::{LocalAllocatorSelector, LocalAllocatorSlot};
#[cfg(nightly_tls_active)]
pub use tls_slot::{ThreadExitReclaim, arm_thread_exit};
pub use usable_size::{thread_allocator_stats, usable_size};

// Re-export internal details used by the macros/internal paths
#[doc(hidden)]
pub use free::{do_local_free_internal, do_local_free_internal_policy};
#[doc(hidden)]
pub use realloc::small_realloc_fits_existing_class;
#[doc(hidden)]
pub use validation::{initialize_allocated_bytes, poison_freed_bytes};

#[doc(hidden)]
pub mod internal;

mod selector;
