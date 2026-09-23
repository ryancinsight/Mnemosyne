//! Telemetry counters and snapshot APIs for OS virtual memory mappings.
//!
//! The recorder is split into a leaf module (`stats`) so the mapping,
//! guard, and reset concerns keep their own hot-path responsibility while the
//! public API remains stable under the crate root.

mod stats;

pub use stats::{BackendMemoryStats, backend_memory_stats};
pub(crate) use stats::{
    record_decommit, record_guard_install, record_map, record_page_reset, record_unmap,
    record_unmap_failure,
};

#[cfg(all(target_os = "linux", not(miri)))]
pub(crate) use stats::{record_hugepage_hint, record_purge_only};
