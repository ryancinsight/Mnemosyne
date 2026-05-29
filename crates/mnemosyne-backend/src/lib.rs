//! Low-level OS page allocation backend mapping interface.

#![no_std]

use core::sync::atomic::{AtomicUsize, Ordering};
#[cfg(target_family = "windows")]
mod windows;
#[cfg(target_family = "windows")]
pub use windows::WindowsBackend as DefaultBackend;

#[cfg(target_family = "unix")]
mod unix;
#[cfg(target_family = "unix")]
pub use unix::UnixBackend as DefaultBackend;

pub mod cuda;
pub use cuda::{is_cuda_available, CudaUnifiedBackend};

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
fn record_map(size: usize) {
    MAP_CALLS.fetch_add(1, Ordering::Relaxed);
    let current = CURRENT_MAPPED_BYTES.fetch_add(size, Ordering::Relaxed) + size;
    if current > PEAK_MAPPED_BYTES.load(Ordering::Relaxed) {
        PEAK_MAPPED_BYTES.fetch_max(current, Ordering::Relaxed);
    }
}

#[inline]
fn record_unmap(size: usize) {
    UNMAP_CALLS.fetch_add(1, Ordering::Relaxed);
    CURRENT_MAPPED_BYTES.fetch_sub(size, Ordering::Relaxed);
}

/// Records an attempted but failed OS release.
///
/// Increments only the call counter so `current_mapped_bytes` stays consistent
/// with the OS-side mapping set when the release call itself failed.
#[inline]
fn record_unmap_failure() {
    UNMAP_CALLS.fetch_add(1, Ordering::Relaxed);
}

/// Records a confirmed page reset.
///
/// Unlike `record_unmap`, this does not decrement `current_mapped_bytes`
/// because the virtual mapping is still committed and remains observable
/// through the allocator's address-space accounting; the OS has only
/// released the underlying physical backing.
#[inline]
fn record_page_reset(size: usize) {
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
fn record_guard_install(size: usize) {
    GUARD_INSTALL_CALLS.fetch_add(1, Ordering::Relaxed);
    GUARD_INSTALL_BYTES.fetch_add(size, Ordering::Relaxed);
}

/// Records a confirmed decommit.
///
/// Same accounting rationale as `record_page_reset`: the reservation remains,
/// so `current_mapped_bytes` is intentionally unchanged; only the commit
/// charge / resident backing was returned to the OS.
#[inline]
fn record_decommit(size: usize) {
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
    fn wrapper_page_reset_round_trips_on_active_mapping() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
        use mnemosyne_core::MemoryBackend;
        // Allocate, reset a sub-range, write through the reset region,
        // and release. Demonstrates that the wrapper telemetry tracks
        // confirmed resets while leaving the mapping committed and
        // writable.
        let size = 64 * 1024;
        // Safety: requesting a multiple of the system page size.
        let ptr = unsafe { MemoryBackendWrapper::allocate(size) };
        assert!(!ptr.is_null());

        let stats_before = backend_memory_stats();
        // Safety: ptr covers `size` bytes; the reset request is valid
        // for the full mapping.
        let reset = unsafe { MemoryBackendWrapper::page_reset(ptr, size) };
        let stats_after = backend_memory_stats();

        if reset {
            assert_eq!(
                stats_after.page_reset_calls,
                stats_before.page_reset_calls + 1
            );
            assert_eq!(
                stats_after.page_reset_bytes,
                stats_before.page_reset_bytes + size
            );
        }
        // current_mapped_bytes must not change regardless of reset outcome.
        assert_eq!(
            stats_after.current_mapped_bytes, stats_before.current_mapped_bytes,
            "page_reset must never alter current_mapped_bytes"
        );

        // The mapping remains writable after reset. Touch a byte to prove
        // the kernel did not unmap the region.
        // Safety: ptr is still a valid committed mapping of `size` bytes.
        unsafe {
            ptr.write_volatile(0xCC);
            assert_eq!(ptr.read_volatile(), 0xCC);
        }

        // Safety: ptr is the exact base of the mapping.
        let released = unsafe { MemoryBackendWrapper::deallocate(ptr, size) };
        assert!(released);
    }

    #[test]
    fn wrapper_page_reset_rejects_null_and_zero() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
        use mnemosyne_core::MemoryBackend;
        let null_reset = unsafe { MemoryBackendWrapper::page_reset(core::ptr::null_mut(), 4096) };
        assert!(!null_reset, "null pointer must not be accepted for reset");

        // Allocate a small mapping just for the size==0 check; size==0 must
        // be rejected before reaching the platform API.
        // Safety: requesting a system-page-aligned size.
        let ptr = unsafe { MemoryBackendWrapper::allocate(4096) };
        assert!(!ptr.is_null());
        let zero_reset = unsafe { MemoryBackendWrapper::page_reset(ptr, 0) };
        assert!(!zero_reset, "zero-size reset must not be accepted");
        let _ = unsafe { MemoryBackendWrapper::deallocate(ptr, 4096) };
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
    fn wrapper_decommit_returns_slack_and_keeps_reservation_releasable() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
        use mnemosyne_core::MemoryBackend;
        // Allocate a multi-page mapping, decommit the trailing half, confirm
        // telemetry, and prove the base reservation still releases cleanly
        // (VirtualFree(MEM_RELEASE) / munmap cover decommitted subranges). The
        // decommitted range is intentionally never touched afterward, since on
        // Windows it faults until re-committed.
        let size = 128 * 1024;
        // Safety: requesting a multiple of the system page size.
        let ptr = unsafe { MemoryBackendWrapper::allocate(size) };
        assert!(!ptr.is_null());

        let half = size / 2;
        // Safety: [ptr + half, ptr + size) is a page-aligned subrange of the
        // mapping holding no live data.
        let tail = unsafe { ptr.add(half) };

        let before = backend_memory_stats();
        // Safety: tail covers `half` bytes inside the live mapping.
        let decommitted = unsafe { MemoryBackendWrapper::decommit(tail, half) };
        let after = backend_memory_stats();

        if decommitted {
            assert_eq!(after.decommit_calls, before.decommit_calls + 1);
            assert_eq!(after.decommit_bytes, before.decommit_bytes + half);
        }
        assert_eq!(
            after.current_mapped_bytes, before.current_mapped_bytes,
            "decommit must never alter current_mapped_bytes"
        );

        // The still-committed first half remains writable.
        // Safety: [ptr, ptr + half) was not decommitted and stays committed.
        unsafe {
            ptr.write_volatile(0x5A);
            assert_eq!(ptr.read_volatile(), 0x5A);
        }

        // The base reservation releases cleanly despite the decommitted tail.
        // Safety: ptr is the exact base of the mapping.
        let released = unsafe { MemoryBackendWrapper::deallocate(ptr, size) };
        assert!(released, "release failed after decommitting a subrange");
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

    #[test]
    fn wrapper_make_guard_records_confirmed_install_and_keeps_mapping_reserved() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
        use mnemosyne_core::MemoryBackend;
        // Allocate, guard the whole region, confirm telemetry, then
        // release. The mapping must still be releasable after a guard
        // install because VirtualFree/munmap only require a valid
        // reservation, not RW access.
        let size = 64 * 1024;
        let ptr = unsafe { MemoryBackendWrapper::allocate(size) };
        assert!(!ptr.is_null());

        // Touch the mapping before guarding to confirm it is initially
        // accessible.
        unsafe {
            ptr.write_volatile(0xAA);
            assert_eq!(ptr.read_volatile(), 0xAA);
        }

        let stats_before = backend_memory_stats();
        let guarded = unsafe { MemoryBackendWrapper::make_guard(ptr, size) };
        let stats_after = backend_memory_stats();

        assert!(
            guarded,
            "make_guard reported failure on a fresh writable mapping"
        );
        assert_eq!(
            stats_after.guard_install_calls,
            stats_before.guard_install_calls + 1
        );
        assert_eq!(
            stats_after.guard_install_bytes,
            stats_before.guard_install_bytes + size
        );
        assert_eq!(
            stats_after.current_mapped_bytes, stats_before.current_mapped_bytes,
            "make_guard must never alter current_mapped_bytes"
        );

        // Release the mapping. VirtualFree(MEM_RELEASE) / munmap accept
        // a region regardless of its protection state.
        let released = unsafe { MemoryBackendWrapper::deallocate(ptr, size) };
        assert!(released);
    }

    #[test]
    fn wrapper_make_guard_rejects_null_and_zero() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
        use mnemosyne_core::MemoryBackend;
        let null_guard = unsafe { MemoryBackendWrapper::make_guard(core::ptr::null_mut(), 4096) };
        assert!(!null_guard, "null pointer must not be accepted for guard");

        let ptr = unsafe { MemoryBackendWrapper::allocate(4096) };
        assert!(!ptr.is_null());
        let zero_guard = unsafe { MemoryBackendWrapper::make_guard(ptr, 0) };
        assert!(!zero_guard, "zero-size guard must not be accepted");
        let _ = unsafe { MemoryBackendWrapper::deallocate(ptr, 4096) };
    }
}

/// High-level OS page mapping backend helper.
pub struct MemoryBackendWrapper;

impl mnemosyne_core::MemoryBackend for MemoryBackendWrapper {
    /// Allocates memory from the OS.
    ///
    /// # Safety
    ///
    /// Size must be greater than zero and page-aligned.
    #[inline(always)]
    unsafe fn allocate(size: usize) -> *mut u8 {
        // Safety: The size must be page-aligned and greater than zero.
        // We forward the allocation request to the target platform's backend safely.
        let ptr = unsafe {
            #[cfg(target_family = "windows")]
            {
                <DefaultBackend as mnemosyne_core::MemoryBackend>::allocate(size)
            }
            #[cfg(target_family = "unix")]
            {
                <DefaultBackend as mnemosyne_core::MemoryBackend>::allocate(size)
            }
            #[cfg(not(any(target_family = "windows", target_family = "unix")))]
            {
                compile_error!("Unsupported target OS family");
            }
        };
        if !ptr.is_null() {
            record_map(size);
        }
        ptr
    }

    /// Releases memory to the OS.
    ///
    /// # Safety
    ///
    /// The ptr must be valid and size must match the allocated size.
    #[inline(always)]
    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        if ptr.is_null() {
            return false;
        }
        // Safety: The ptr must be valid and size must match the allocated size.
        // We forward the deallocation request to the target platform's backend
        // safely and only record an "unmapped bytes" delta when the OS release
        // confirms success, so the live mapping set always agrees with
        // `current_mapped_bytes`.
        let released = unsafe {
            #[cfg(target_family = "windows")]
            {
                <DefaultBackend as mnemosyne_core::MemoryBackend>::deallocate(ptr, size)
            }
            #[cfg(target_family = "unix")]
            {
                <DefaultBackend as mnemosyne_core::MemoryBackend>::deallocate(ptr, size)
            }
            #[cfg(not(any(target_family = "windows", target_family = "unix")))]
            {
                compile_error!("Unsupported target OS family")
            }
        };
        if released {
            record_unmap(size);
        } else {
            record_unmap_failure();
        }
        released
    }

    /// Drops the physical backing of an idle page range while keeping the
    /// virtual mapping committed. Telemetry records confirmed resets only
    /// (call count and byte count); `current_mapped_bytes` is intentionally
    /// not decremented because the address space remains owned by the
    /// allocator.
    #[inline(always)]
    unsafe fn page_reset(ptr: *mut u8, size: usize) -> bool {
        if ptr.is_null() || size == 0 {
            return false;
        }
        // Safety: caller upholds the per-platform page_reset contract.
        let reset =
            unsafe { <DefaultBackend as mnemosyne_core::MemoryBackend>::page_reset(ptr, size) };
        if reset {
            record_page_reset(size);
        }
        reset
    }

    /// Installs a `PROT_NONE` / `PAGE_NOACCESS` guard region on an
    /// active mapping. Telemetry records confirmed installs only
    /// (`guard_install_calls`, `guard_install_bytes`);
    /// `current_mapped_bytes` is intentionally not decremented because
    /// the mapping remains reserved.
    #[inline(always)]
    unsafe fn make_guard(ptr: *mut u8, size: usize) -> bool {
        if ptr.is_null() || size == 0 {
            return false;
        }
        // Safety: caller upholds the per-platform make_guard contract.
        let guarded =
            unsafe { <DefaultBackend as mnemosyne_core::MemoryBackend>::make_guard(ptr, size) };
        if guarded {
            record_guard_install(size);
        }
        guarded
    }

    /// Releases the commit charge / resident backing of a page-aligned range
    /// while keeping the reservation. Telemetry records confirmed decommits
    /// only (`decommit_calls`, `decommit_bytes`); `current_mapped_bytes` is
    /// intentionally not decremented because the address space stays reserved
    /// until `deallocate`.
    #[inline(always)]
    unsafe fn decommit(ptr: *mut u8, size: usize) -> bool {
        if ptr.is_null() || size == 0 {
            return false;
        }
        // Safety: caller upholds the per-platform decommit contract.
        let decommitted =
            unsafe { <DefaultBackend as mnemosyne_core::MemoryBackend>::decommit(ptr, size) };
        if decommitted {
            record_decommit(size);
        }
        decommitted
    }
}
