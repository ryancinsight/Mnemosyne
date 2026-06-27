//! Guard-region installation, the `PROT_NONE` / `PAGE_NOACCESS` concern.
//!
//! Provides `do_make_guard` — the per-method body that the
//! [`MemoryBackend::make_guard`] impl-block in [`crate::mapping`]
//! delegates into. Confirmed installs are forwarded to
//! `crate::recorders::record_guard_install` for telemetry.
//!
//! Wrapper composite tests live here because they anchor the guard
//! concern end-to-end (allocate → make_guard → write survives → release).

use crate::recorders::record_guard_install;
use mnemosyne_core::MemoryBackend;

/// Performs the make-guard work for [`crate::mapping::MemoryBackendWrapper`].
/// Delegates to `B::make_guard`; on a confirmed install, records the
/// outcome through [`crate::recorders::record_guard_install`].
///
/// `#[inline(always)]` keeps the helper statically dispatched at the
/// call site.
#[inline(always)]
pub(crate) fn do_make_guard<B: MemoryBackend>(ptr: *mut u8, size: usize) -> bool {
    if ptr.is_null() || size == 0 {
        return false;
    }
    // Safety: caller upholds the per-platform make_guard contract.
    let guarded = unsafe { B::make_guard(ptr, size) };
    if guarded {
        record_guard_install(size);
    }
    guarded
}

#[cfg(test)]
mod tests {
    extern crate std;

    use crate::mapping::MemoryBackendWrapper;
    use crate::recorders::backend_memory_stats;
    use mnemosyne_core::MemoryBackend;

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn wrapper_make_guard_records_confirmed_install_and_keeps_mapping_reserved() {
        let _guard = TEST_LOCK
            .lock()
            .expect("backend telemetry test lock was poisoned");
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
        let null_guard = unsafe { MemoryBackendWrapper::make_guard(core::ptr::null_mut(), 4096) };
        assert!(!null_guard, "null pointer must not be accepted for guard");

        let ptr = unsafe { MemoryBackendWrapper::allocate(4096) };
        assert!(!ptr.is_null());
        let zero_guard = unsafe { MemoryBackendWrapper::make_guard(ptr, 0) };
        assert!(!zero_guard, "zero-size guard must not be accepted");
        let _ = unsafe { MemoryBackendWrapper::deallocate(ptr, 4096) };
    }
}
