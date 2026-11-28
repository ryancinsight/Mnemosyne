//! Per-size-class allocation telemetry.
//!
//! Tracks process-wide allocation and deallocation counts per size class using
//! relaxed atomic counters. The allocation-byte total is derived from the
//! immutable class stride. Per-thread counters batch global updates so the
//! allocator hot path does not contend on one cache line for every operation.
//!
//! The counters are always enabled (no feature gate). They are not
//! synchronised with each other — a snapshot can observe counts from
//! different points in time — and pending per-thread batches may not yet be
//! visible. Each global counter is monotone-non-decreasing, so the
//! live-allocation estimate `alloc_count - dealloc_count` is an under-estimate
//! until the owning thread flushes its pending batch rather than a negative
//! artefact.
//!
//! ## Reset boundary (generation counter)
//!
//! `reset_bin_stats()` increments a process-wide `RESET_GENERATION` counter.
//! Each TLS batch records the generation it was started in. When a batch flush
//! sees that the global generation has advanced past its recorded generation,
//! it silently discards the batch instead of adding stale observations to the
//! fresh counters. This ensures that every worker's pre-reset activity is
//! excluded from post-reset snapshots regardless of flush ordering.
//!
//! Fragmentation ratio per class: `(alloc_bytes - dealloc_bytes) /
//! alloc_bytes`. Internal fragmentation per class: `(alloc_bytes -
//! requested_bytes) / alloc_bytes`.
//!
//! The public reading API and its tests live in the private `api` module;
//! this file stays a manifest of the module tree and its curated re-exports.

mod api;
mod batch;
mod snapshot;

pub use api::{
    all_bin_snapshots, alloc_distribution, bin_snapshot, flush_tls_stats, hottest_class,
    reset_bin_stats, reset_generation_count, summary_line, total_alloc_count,
    total_internal_fragmentation, total_live_bytes, total_requested_bytes,
};
pub(crate) use batch::{record_alloc_with_size, record_dealloc};
pub use snapshot::BinSnapshot;
