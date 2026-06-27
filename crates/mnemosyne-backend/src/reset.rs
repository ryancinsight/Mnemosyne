//! Physical-backing release, the page-reset + decommit concern.
//!
//! Provides `do_page_reset` and `do_decommit` — the per-method
//! bodies that the [`MemoryBackend::page_reset`] and
//! [`MemoryBackend::decommit`] impl-block entries in
//! [`crate::mapping`] delegate into. Both share the same accounting:
//! a call + byte counter and no `current_mapped_bytes` delta, because
//! the virtual address space remains reserved until
//! [`crate::mapping::MemoryBackendWrapper::deallocate`].
//!
//! [`MemoryBackend::page_reset`]: mnemosyne_core::MemoryBackend::page_reset
//! [`MemoryBackend::decommit`]: mnemosyne_core::MemoryBackend::decommit
//!
//! Wrapper composite tests live here because they anchor the reset
//! concern end-to-end (allocate → page_reset/decommit → write
//! survives → release).

use crate::recorders::{record_decommit, record_page_reset};
use mnemosyne_core::MemoryBackend;

/// Performs the page-reset work for [`crate::mapping::MemoryBackendWrapper`].
/// Delegates to `B::page_reset`; on a confirmed reset, records the
/// outcome through [`crate::recorders::record_page_reset`].
///
/// `#[inline(always)]` keeps the helper statically dispatched at the
/// call site.
#[inline(always)]
pub(crate) fn do_page_reset<B: MemoryBackend>(ptr: *mut u8, size: usize) -> bool {
    if ptr.is_null() || size == 0 {
        return false;
    }
    let reset = unsafe { B::page_reset(ptr, size) };
    if reset {
        record_page_reset(size);
    }
    reset
}

/// Performs the decommit work for [`crate::mapping::MemoryBackendWrapper`].
/// Delegates to `B::decommit`; on a confirmed decommit, records the
/// outcome through [`crate::recorders::record_decommit`].
///
/// `#[inline(always)]` keeps the helper statically dispatched at the
/// call site.
#[inline(always)]
pub(crate) fn do_decommit<B: MemoryBackend>(ptr: *mut u8, size: usize) -> bool {
    if ptr.is_null() || size == 0 {
        return false;
    }
    let decommitted = unsafe { B::decommit(ptr, size) };
    if decommitted {
        record_decommit(size);
    }
    decommitted
}

#[cfg(test)]
mod tests {
    extern crate std;

    use crate::mapping::MemoryBackendWrapper;
    use crate::recorders::backend_memory_stats;
    use mnemosyne_core::MemoryBackend;

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn wrapper_page_reset_round_trips_on_active_mapping() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
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
    fn wrapper_decommit_returns_slack_and_keeps_reservation_releasable() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
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
}
