//! Core allocator types, constants, size classes, and synchronization primitives for Mnemosyne.

#![no_std]

pub mod constants;
pub mod policy;
pub mod size_class;
pub mod sync;
pub mod types;
pub mod validation;

pub use constants::*;
pub use policy::{AllocPolicy, SecurePolicy, StandardPolicy};
pub use size_class::*;
pub use sync::*;
pub use types::*;
pub use validation::{is_valid_alloc_request, is_valid_layout_alloc_request};

/// Trait defining the contract for low-level virtual memory mapping backends.
pub trait MemoryBackend: Send + Sync + 'static {
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
}
