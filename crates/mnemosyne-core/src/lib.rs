//! Core allocator types, constants, size classes, and synchronization primitives for Mnemosyne.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod constants;
pub mod policy;
pub mod size_class;
pub mod sync;
pub mod types;
pub mod options;
pub mod validation;

pub use constants::*;
pub use policy::{AllocPolicy, HardenedPolicy, SecurePolicy, StandardPolicy};
pub use size_class::*;
pub use sync::*;
pub use types::*;
pub use validation::{is_valid_alloc_request, is_valid_layout_alloc_request};

/// Trait defining the contract for low-level virtual memory mapping backends.
pub trait MemoryBackend: Send + Sync + 'static {
    /// Indicates whether the backend supports advisory page resetting.
    const SUPPORTS_PAGE_RESET: bool = false;

    /// Indicates whether the backend supports page protection guard installation.
    const SUPPORTS_MAKE_GUARD: bool = false;

    /// Indicates whether the backend supports releasing memory commitment while keeping the reservation.
    const SUPPORTS_DECOMMIT: bool = false;

    /// Indicates whether the backend enables the lock-free per-CPU block cache.
    const ENABLE_CPU_CACHE: bool = false;

    /// Allocates page-aligned memory from the OS.
    ///
    /// # Safety
    ///
    /// The size must be greater than zero and page-aligned.
    unsafe fn allocate(size: usize) -> *mut u8;

    /// Releases page-aligned memory back to the OS.
    ///
    /// Returns `true` when the OS confirmed the release, `false` when the
    /// release call reported failure. Callers must defer telemetry that
    /// observes "unmapped bytes" to a `true` outcome to keep the
    /// `current_mapped_bytes` counter consistent with the live mapping set.
    /// Cleanup-path callers that have no recovery action available for a
    /// failed release must still bind the result with `let _released =` and
    /// document why the leaked mapping is unrecoverable in that context.
    ///
    /// # Safety
    ///
    /// The ptr must be valid and size must match the allocated size.
    #[must_use = "ignoring the release result drops the OS-failure signal; bind it to `_released` and document why no recovery is possible"]
    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool;

    /// Asks the OS to drop the physical backing of a mapped page range while
    /// keeping the virtual address range reserved and accessible.
    ///
    /// Returns `true` when the OS confirmed the reset, `false` when the
    /// backend either does not implement page-level reset, the call failed,
    /// or the platform's reset semantics are too lax for this allocator's
    /// purposes (for example, `MADV_FREE` on a backend that requires
    /// observable zeroing).
    ///
    /// The reset is *advisory at the address-space level* — the mapping
    /// remains readable and writable after the call. Subsequent reads may
    /// return zeroed pages (Linux `MADV_DONTNEED`, Windows
    /// `VirtualAlloc(MEM_RESET)` after touch) or the previous contents
    /// until the next write (macOS `MADV_FREE`). Callers must therefore
    /// treat the contents of the reset region as undefined.
    ///
    /// The default implementation returns `false` so a backend that has no
    /// equivalent operation (such as the CUDA unified memory backend)
    /// silently opts out without breaking the trait surface.
    ///
    /// # Safety
    ///
    /// `ptr` must be a system-page-aligned address inside an active mapping
    /// from this backend, `size` must be a non-zero multiple of the system
    /// page size, and `[ptr, ptr + size)` must lie entirely within a single
    /// allocation returned by `allocate`. After a successful reset the
    /// region may be re-faulted by the kernel; callers must not assume the
    /// previous bytes are still present.
    #[allow(unused_variables)]
    unsafe fn page_reset(ptr: *mut u8, size: usize) -> bool {
        false
    }

    /// Marks a page-aligned range as an inaccessible guard region.
    ///
    /// On Unix the implementation calls `mprotect(ptr, size, PROT_NONE)`;
    /// on Windows it calls `VirtualProtect(ptr, size, PAGE_NOACCESS, _)`.
    /// Either flavor leaves the address range mapped (so subsequent
    /// `deallocate` calls still cover it) but raises a fault on any read
    /// or write. The default implementation returns `false` so backends
    /// without an equivalent operation silently opt out.
    ///
    /// Callers should treat the guard as one-way: there is no
    /// corresponding "remove guard" operation in this trait. A backend
    /// that needs to reuse the range must release the entire mapping via
    /// `deallocate` and re-allocate.
    ///
    /// # Safety
    ///
    /// `ptr` must be a system-page-aligned address inside an active
    /// mapping from this backend, `size` must be a non-zero multiple of
    /// the system page size, and `[ptr, ptr + size)` must lie entirely
    /// within a single allocation returned by `allocate`. After a
    /// successful guard install, every read or write to the range raises
    /// the platform's protection fault — callers must ensure no live
    /// allocator data lives in the range.
    #[allow(unused_variables)]
    unsafe fn make_guard(ptr: *mut u8, size: usize) -> bool {
        false
    }

    /// Releases the commit charge / physical backing of a page-aligned range
    /// while keeping the surrounding reservation intact, so the range can still
    /// be covered by the eventual `deallocate` of the base allocation.
    ///
    /// This differs from `page_reset`: `page_reset` keeps the range committed
    /// (Windows `MEM_RESET` only discards contents; the pages still count
    /// against the commit limit), whereas `decommit` actually releases the
    /// commitment — Windows `VirtualFree(MEM_DECOMMIT)` drops the commit charge,
    /// and Unix `madvise(MADV_DONTNEED)` drops the resident pages. It is used to
    /// return the alignment slack that aligned segment/huge mappings reserve but
    /// never touch (on Windows that slack is eagerly committed and would
    /// otherwise hold ~`SEGMENT_ALIGN` of commit charge per mapping).
    ///
    /// The default implementation returns `false` so backends without an
    /// equivalent operation silently opt out.
    ///
    /// # Safety
    ///
    /// `ptr` must be a system-page-aligned address inside an active mapping from
    /// this backend, `size` must be a non-zero multiple of the system page size,
    /// and `[ptr, ptr + size)` must lie entirely within a single allocation
    /// returned by `allocate` **and** must hold no live allocator data — after a
    /// successful decommit the range faults on access until re-committed or
    /// released.
    #[allow(unused_variables)]
    unsafe fn decommit(ptr: *mut u8, size: usize) -> bool {
        false
    }
}
