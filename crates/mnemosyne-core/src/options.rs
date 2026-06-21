//! Global thread-safe runtime configuration options.
//!
//! These options are populated once at startup from environment variables
//! and accessed via relaxed atomic reads throughout the allocator.

use core::sync::atomic::{AtomicBool, AtomicUsize};

/// The maximum number of segments retained in the global segment pool.
pub static MAX_RETAINED_SEGMENTS: AtomicUsize = AtomicUsize::new(crate::constants::MAX_RETAINED_SEGMENTS_LIMIT);


/// Whether the advisory huge page hint (`MADV_HUGEPAGE`) is enabled on Linux.
pub static ENABLE_HUGEPAGE_HINT: AtomicBool = AtomicBool::new(true);

/// The cadence in milliseconds at which retained segments are purged in the background.
pub static PURGE_CADENCE_MS: AtomicUsize = AtomicUsize::new(0);

/// Runtime configuration options for the Mnemosyne allocator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MnemosyneOptions {
    pub max_retained_segments: usize,
    pub purge_cadence_ms: usize,
    pub enable_hugepage_hint: bool,
}

impl Default for MnemosyneOptions {
    #[inline]
    fn default() -> Self {
        Self {
            max_retained_segments: crate::constants::MAX_RETAINED_SEGMENTS_LIMIT,
            purge_cadence_ms: 0,
            enable_hugepage_hint: true,
        }
    }
}

/// Returns the current runtime configuration options snapshot.
#[inline]
pub fn get_options() -> MnemosyneOptions {
    use core::sync::atomic::Ordering;
    MnemosyneOptions {
        max_retained_segments: MAX_RETAINED_SEGMENTS.load(Ordering::Acquire),
        purge_cadence_ms: PURGE_CADENCE_MS.load(Ordering::Acquire),
        enable_hugepage_hint: ENABLE_HUGEPAGE_HINT.load(Ordering::Acquire),
    }
}

/// Overwrites the runtime configuration options.
#[inline]
pub fn set_options(options: MnemosyneOptions) {
    use core::sync::atomic::Ordering;
    let clamped_retained = core::cmp::min(
        options.max_retained_segments,
        crate::constants::MAX_RETAINED_SEGMENTS_LIMIT,
    );
    MAX_RETAINED_SEGMENTS.store(clamped_retained, Ordering::Release);
    PURGE_CADENCE_MS.store(options.purge_cadence_ms, Ordering::Release);
    ENABLE_HUGEPAGE_HINT.store(options.enable_hugepage_hint, Ordering::Release);
}
