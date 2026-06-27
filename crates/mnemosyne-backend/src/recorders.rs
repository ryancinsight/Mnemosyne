//! Telemetry and stats tracking for OS virtual memory mappings.
//!
//! Pure recorder counters and the per-concern unit tests for `record_*`.
//! The recorder is exposed as `pub(crate)` so sibling concern modules
//! ([`crate::mapping`], [`crate::guard`], [`crate::reset`]) can update
//! counters on confirmed OS outcomes; external consumers reach the
//! snapshot through [`backend_memory_stats`] and [`BackendMemoryStats`].

use core::sync::atomic::{AtomicUsize, Ordering};

static CURRENT_MAPPED_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_MAPPED_BYTES: AtomicUsize = AtomicUsize::new(0);
static MAP_CALLS: AtomicUsize = AtomicUsize::new(0);
static UNMAP_CALLS: AtomicUsize = AtomicUsize::new(0);
static PAGE_RESET_CALLS: AtomicUsize = AtomicUsize::new(0);
static PAGE_RESET_BYTES: AtomicUsize = AtomicUsize::new(0);
static GUARD_INSTALL_CALLS: AtomicUsize = AtomicUsize::new(0);
static GUARD_INSTALL_BYTES: AtomicUsize = AtomicUsize::new(0);
static DECOMMIT_CALLS: AtomicUsize = AtomicUsize::new(0);
static DECOMMIT_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Snapshot of OS mappings requested by Mnemosyne.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BackendMemoryStats {
    pub current_mapped_bytes: usize,
    pub peak_mapped_bytes: usize,
    pub map_calls: usize,
    pub unmap_calls: usize,
    /// Number of `page_reset` calls that the OS confirmed.
    ///
    /// A reset releases the physical backing of an addressed range while
    /// keeping the virtual mapping intact, so this counter is independent
    /// of `unmap_calls` and `current_mapped_bytes` is not decremented.
    pub page_reset_calls: usize,
    /// Cumulative byte count passed to confirmed `page_reset` calls.
    pub page_reset_bytes: usize,
    /// Number of `make_guard` calls the OS confirmed.
    ///
    /// A guard install changes only the protection bits of an addressed
    /// range; the mapping remains reserved, so this counter is
    /// independent of `unmap_calls` and `current_mapped_bytes` is not
    /// decremented.
    pub guard_install_calls: usize,
    /// Cumulative byte count passed to confirmed `make_guard` calls.
    pub guard_install_bytes: usize,
    /// Number of `decommit` calls the OS confirmed.
    ///
    /// A decommit releases the commit charge / resident backing of an addressed
    /// range while keeping the reservation, so this counter is independent of
    /// `unmap_calls` and `current_mapped_bytes` is not decremented (the address
    /// space remains reserved until `deallocate`).
    pub decommit_calls: usize,
    /// Cumulative byte count passed to confirmed `decommit` calls (the commit
    /// charge / resident backing returned to the OS).
    pub decommit_bytes: usize,
}

#[inline]
pub(crate) fn record_map(size: usize) {
    MAP_CALLS.fetch_add(1, Ordering::Relaxed);
    let current = CURRENT_MAPPED_BYTES.fetch_add(size, Ordering::Relaxed) + size;
    if current > PEAK_MAPPED_BYTES.load(Ordering::Relaxed) {
        PEAK_MAPPED_BYTES.fetch_max(current, Ordering::Relaxed);
    }
}

#[inline]
pub(crate) fn record_unmap(size: usize) {
    UNMAP_CALLS.fetch_add(1, Ordering::Relaxed);
    CURRENT_MAPPED_BYTES.fetch_sub(size, Ordering::Relaxed);
}

/// Records an attempted but failed OS release.
///
/// Increments only the call counter so `current_mapped_bytes` stays consistent
/// with the OS-side mapping set when the release call itself failed.
#[inline]
pub(crate) fn record_unmap_failure() {
    UNMAP_CALLS.fetch_add(1, Ordering::Relaxed);
}

/// Records a confirmed page reset.
///
/// Unlike `record_unmap`, this does not decrement `current_mapped_bytes`
/// because the virtual mapping is still committed and remains observable
/// through the allocator's address-space accounting; the OS has only
/// released the underlying physical backing.
#[inline]
pub(crate) fn record_page_reset(size: usize) {
    PAGE_RESET_CALLS.fetch_add(1, Ordering::Relaxed);
    PAGE_RESET_BYTES.fetch_add(size, Ordering::Relaxed);
}

/// Records a confirmed guard-region install.
///
/// Same accounting rationale as `record_page_reset`: the mapping remains
/// reserved and `current_mapped_bytes` is intentionally unchanged. The
/// counter increments lets external monitors observe how much of the
/// reserved address space has been converted into guard regions.
#[inline]
pub(crate) fn record_guard_install(size: usize) {
    GUARD_INSTALL_CALLS.fetch_add(1, Ordering::Relaxed);
    GUARD_INSTALL_BYTES.fetch_add(size, Ordering::Relaxed);
}

/// Records a confirmed decommit.
///
/// Same accounting rationale as `record_page_reset`: the reservation remains,
/// so `current_mapped_bytes` is intentionally unchanged; only the commit
/// charge / resident backing was returned to the OS.
#[inline]
pub(crate) fn record_decommit(size: usize) {
    DECOMMIT_CALLS.fetch_add(1, Ordering::Relaxed);
    DECOMMIT_BYTES.fetch_add(size, Ordering::Relaxed);
}

/// Returns the current backend memory mapping counters.
///
/// The snapshot uses relaxed atomics because these counters are telemetry only:
/// allocator correctness never depends on cross-counter synchronization.
pub fn backend_memory_stats() -> BackendMemoryStats {
    BackendMemoryStats {
        current_mapped_bytes: CURRENT_MAPPED_BYTES.load(Ordering::Relaxed),
        peak_mapped_bytes: PEAK_MAPPED_BYTES.load(Ordering::Relaxed),
        map_calls: MAP_CALLS.load(Ordering::Relaxed),
        unmap_calls: UNMAP_CALLS.load(Ordering::Relaxed),
        page_reset_calls: PAGE_RESET_CALLS.load(Ordering::Relaxed),
        page_reset_bytes: PAGE_RESET_BYTES.load(Ordering::Relaxed),
        guard_install_calls: GUARD_INSTALL_CALLS.load(Ordering::Relaxed),
        guard_install_bytes: GUARD_INSTALL_BYTES.load(Ordering::Relaxed),
        decommit_calls: DECOMMIT_CALLS.load(Ordering::Relaxed),
        decommit_bytes: DECOMMIT_BYTES.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn mapping_telemetry_tracks_deltas_and_peak() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
        let before = backend_memory_stats();
        let size = 4096;

        record_map(size);
        let during = backend_memory_stats();

        assert_eq!(
            during.current_mapped_bytes,
            before.current_mapped_bytes + size
        );
        assert_eq!(during.map_calls, before.map_calls + 1);
        assert_eq!(during.unmap_calls, before.unmap_calls);
        assert!(
            during.peak_mapped_bytes >= during.current_mapped_bytes,
            "peak {} below current {} after record_map",
            during.peak_mapped_bytes,
            during.current_mapped_bytes
        );
        assert!(
            during.peak_mapped_bytes >= before.peak_mapped_bytes,
            "peak {} below pre-map peak {}",
            during.peak_mapped_bytes,
            before.peak_mapped_bytes
        );

        record_unmap(size);
        let after = backend_memory_stats();

        assert_eq!(after.current_mapped_bytes, before.current_mapped_bytes);
        assert_eq!(after.map_calls, before.map_calls + 1);
        assert_eq!(after.unmap_calls, before.unmap_calls + 1);
        assert!(
            after.peak_mapped_bytes >= during.peak_mapped_bytes,
            "peak {} regressed below mid-cycle peak {}",
            after.peak_mapped_bytes,
            during.peak_mapped_bytes
        );
    }

    #[test]
    fn failed_release_increments_call_count_without_byte_delta() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
        let before = backend_memory_stats();
        let size = 4096;

        record_map(size);
        let mapped = backend_memory_stats();
        assert_eq!(
            mapped.current_mapped_bytes,
            before.current_mapped_bytes + size
        );

        // Simulate a failed OS release: the wrapper increments the call counter
        // but must not subtract bytes that remain mapped from the OS perspective.
        record_unmap_failure();
        let failed = backend_memory_stats();
        assert_eq!(failed.current_mapped_bytes, mapped.current_mapped_bytes);
        assert_eq!(failed.unmap_calls, mapped.unmap_calls + 1);
        assert_eq!(failed.map_calls, mapped.map_calls);

        record_unmap(size);
        let cleared = backend_memory_stats();
        assert_eq!(cleared.current_mapped_bytes, before.current_mapped_bytes);
    }

    #[test]
    fn page_reset_telemetry_increments_call_and_byte_counters_only() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
        // record_page_reset must increment both call and byte counters
        // without touching current_mapped_bytes, because a reset releases
        // physical backing while leaving the virtual mapping committed.
        let before = backend_memory_stats();
        let size = 8192;

        record_page_reset(size);
        let after = backend_memory_stats();

        assert_eq!(
            after.page_reset_calls,
            before.page_reset_calls + 1,
            "page_reset_calls counter did not advance"
        );
        assert_eq!(
            after.page_reset_bytes,
            before.page_reset_bytes + size,
            "page_reset_bytes counter did not advance by the reset size"
        );
        assert_eq!(
            after.current_mapped_bytes, before.current_mapped_bytes,
            "page_reset must not decrement current_mapped_bytes"
        );
        assert_eq!(
            after.unmap_calls, before.unmap_calls,
            "page_reset must not increment unmap_calls"
        );
    }

    #[test]
    fn decommit_telemetry_increments_call_and_byte_counters_only() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
        // record_decommit must increment both counters without touching
        // current_mapped_bytes (the reservation persists) or the unmap/reset
        // counters.
        let before = backend_memory_stats();
        let size = 64 * 1024;

        record_decommit(size);
        let after = backend_memory_stats();

        assert_eq!(
            after.decommit_calls,
            before.decommit_calls + 1,
            "decommit_calls counter did not advance"
        );
        assert_eq!(
            after.decommit_bytes,
            before.decommit_bytes + size,
            "decommit_bytes counter did not advance by the decommit size"
        );
        assert_eq!(
            after.current_mapped_bytes, before.current_mapped_bytes,
            "decommit must not decrement current_mapped_bytes (reservation persists)"
        );
        assert_eq!(after.unmap_calls, before.unmap_calls);
        assert_eq!(after.page_reset_calls, before.page_reset_calls);
    }

    #[test]
    fn guard_telemetry_increments_call_and_byte_counters_only() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
        // record_guard_install must increment both counters without
        // perturbing current_mapped_bytes, page_reset, or unmap counters.
        let before = backend_memory_stats();
        let size = 4096;
        record_guard_install(size);
        let after = backend_memory_stats();
        assert_eq!(
            after.guard_install_calls,
            before.guard_install_calls + 1,
            "guard_install_calls counter did not advance"
        );
        assert_eq!(
            after.guard_install_bytes,
            before.guard_install_bytes + size,
            "guard_install_bytes counter did not advance by the guard size"
        );
        assert_eq!(
            after.current_mapped_bytes, before.current_mapped_bytes,
            "make_guard must not decrement current_mapped_bytes"
        );
        assert_eq!(after.page_reset_calls, before.page_reset_calls);
        assert_eq!(after.unmap_calls, before.unmap_calls);
    }
}
