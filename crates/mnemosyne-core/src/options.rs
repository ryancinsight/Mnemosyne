//! Global thread-safe runtime configuration options.
//!
//! These options are populated once at startup from environment variables
//! and accessed via relaxed atomic reads throughout the allocator.

use core::sync::atomic::{AtomicBool, AtomicUsize};

/// The maximum number of segments retained in the global segment pool.
pub static MAX_RETAINED_SEGMENTS: AtomicUsize = AtomicUsize::new(32);

/// Whether the advisory huge page hint (`MADV_HUGEPAGE`) is enabled on Linux.
pub static ENABLE_HUGEPAGE_HINT: AtomicBool = AtomicBool::new(true);

/// The cadence in milliseconds at which retained segments are purged in the background.
pub static PURGE_CADENCE_MS: AtomicUsize = AtomicUsize::new(0);
