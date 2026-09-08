//! Allocator statistics: the snapshot type, live queries, retained-pool
//! maintenance, and the reporting surface.

mod maintenance;
mod query;
mod reporting;
mod types;

pub use maintenance::{
    decay, purge, purge_generic, purge_lazy, purge_standard, reset, reset_generic,
};
pub use query::{memory_stats, memory_stats_generic, memory_stats_json};
pub use reporting::{BinStatsWindow, policy_summary, top_n_classes};
pub use types::MemoryStats;
