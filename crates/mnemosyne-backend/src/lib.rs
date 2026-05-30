//! Low-level OS page allocation backend mapping interface.

#![no_std]

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

pub mod telemetry;
pub use telemetry::{backend_memory_stats, BackendMemoryStats};

/// High-level OS page mapping backend helper.
pub struct MemoryBackendWrapper;

impl mnemosyne_core::MemoryBackend for MemoryBackendWrapper {
    const SUPPORTS_PAGE_RESET: bool = DefaultBackend::SUPPORTS_PAGE_RESET;
    const SUPPORTS_MAKE_GUARD: bool = DefaultBackend::SUPPORTS_MAKE_GUARD;
    const SUPPORTS_DECOMMIT: bool = DefaultBackend::SUPPORTS_DECOMMIT;
    const ENABLE_CPU_CACHE: bool = DefaultBackend::ENABLE_CPU_CACHE;

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
            telemetry::record_map(size);
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
            telemetry::record_unmap(size);
        } else {
            telemetry::record_unmap_failure();
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
            telemetry::record_page_reset(size);
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
            telemetry::record_guard_install(size);
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
            telemetry::record_decommit(size);
        }
        decommitted
    }
}
