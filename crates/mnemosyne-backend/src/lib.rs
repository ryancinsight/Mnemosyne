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

/// Snapshot of OS mappings requested by Mnemosyne.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BackendMemoryStats {
    pub current_mapped_bytes: usize,
    pub peak_mapped_bytes: usize,
    pub map_calls: usize,
    pub unmap_calls: usize,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_telemetry_tracks_deltas_and_peak() {
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
        assert!(during.peak_mapped_bytes >= during.current_mapped_bytes);
        assert!(during.peak_mapped_bytes >= before.peak_mapped_bytes);

        record_unmap(size);
        let after = backend_memory_stats();

        assert_eq!(after.current_mapped_bytes, before.current_mapped_bytes);
        assert_eq!(after.map_calls, before.map_calls + 1);
        assert_eq!(after.unmap_calls, before.unmap_calls + 1);
        assert!(after.peak_mapped_bytes >= during.peak_mapped_bytes);
    }

    #[test]
    fn failed_release_increments_call_count_without_byte_delta() {
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
}
