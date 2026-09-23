//! DIP boundary traits for thread-local allocator cache selection.
//!
//! These traits are the dependency-inversion boundary between the allocator
//! engine and the TLS/policy infrastructure:
//!
//! - [`PolicySlotSelection<B>`] — per-policy TLS slot access interface.
//! - [`LocalAllocatorSelector<B>`] — per-backend TLS cache selection interface.
//!
//! Both are implemented by the `impl_local_allocator_selector!` expansion
//! in [`crate::selector`]; the allocation/free hot paths call through them.

use crate::local_alloc::ThreadAllocator;
use mnemosyne_arena::HasSegmentPool;
/// Trait resolving dynamic backend-specific thread-local cache selection.
///
/// Implementors provide independent cache state for the standard and
/// encrypted free-list modes. The exported selector macro is the canonical
/// implementation; custom implementations must preserve the same ownership
/// and mode-isolation contract.
pub trait PolicySlotSelection<B: HasSegmentPool> {
    /// Runs `f` with access to the allocator cache selected for this policy.
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R>;

    /// Runs `f` with access to the selected policy cache without arming the
    /// re-entrancy gate.
    ///
    /// # Safety
    /// The caller must guarantee `f` performs no allocation or deallocation
    /// through this same policy's slot: the re-entrancy gate that would catch
    /// such a call is deliberately not armed, so re-entry aliases the cached
    /// `&mut ThreadAllocator`.
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R>;

    /// Returns the initialized allocator pointer for this policy's TLS slot.
    fn get_allocator_ptr() -> *mut core::ffi::c_void;

    /// Returns the raw allocator pointer for this policy's TLS slot without
    /// forcing lazy initialization.
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void;
}

/// Backend hook that supplies the thread-local allocator cache and mode-keyed
/// selector entry points for a concrete segment pool backend.
/**
The backend-local thread allocator selection API.

Each backend owns its own TLS allocator cache, and each policy mode further
selects the matching allocator for that backend. Implementations must keep the
policy-keyed TLS slot stable for the lifetime of the thread and must preserve
mode isolation between standard and encrypted free-list chains.
*/
/// Backend hook that supplies the thread-local allocator cache and mode-keyed
/// selector entry points for a concrete segment pool backend.
///
/// Each backend chooses the thread-local cache and policy-specific slot layout it
/// needs while preserving the same ownership and re-entrancy guarantees for the
/// allocator engine.
pub trait LocalAllocatorSelector<B: HasSegmentPool>: HasSegmentPool {
    /// Evaluates the closure with a mutable reference to the thread-local allocator cache,
    /// arming the re-entrancy guard.
    ///
    /// Returns `None` if the allocator is already borrowed (re-entrancy detected).
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R>;

    /// Runs `f` with the thread-local allocator cache **without** arming the
    /// re-entrancy guard, returning `None` on same-thread re-entry.
    ///
    /// This backs the guard-free small-allocation fast path: it still consults
    /// the re-entrancy busy bit (so it never produces a second `&mut` while a
    /// guarded borrow is live) but skips the guard set/clear writes.
    ///
    /// # Safety
    ///
    /// `f` must not, directly or transitively, invoke any allocator entry point
    /// on the current thread.
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R>;

    /// Returns the raw pointer to the thread-local allocator cache.
    fn get_allocator_ptr() -> *mut core::ffi::c_void;

    /// Returns the raw pointer to the thread-local allocator cache without triggering lazy initialization.
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void;

    /// Registers a manually created allocator instance as the current thread's
    /// active cache pointer for this backend mode so direct
    /// `ThreadAllocator::new()` uses remain compatible with the public free path.
    fn register_current_allocator_ptr(ptr: *mut core::ffi::c_void);

    /// Runs `f` against the TLS allocator selected by the compile-time
    /// free-list encryption mode.
    ///
    /// The mode is part of the selector call, not process-global state. This
    /// gives each `(backend, encryption mode)` pair an independent allocator
    /// cache while preserving static dispatch and the existing backend seam.
    fn with_allocator_for_policy<P, R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R>
    where
        P: mnemosyne_core::AllocPolicy + PolicySlotSelection<B>;

    /// Mode-keyed counterpart of [`Self::with_allocator_unguarded`].
    ///
    /// # Safety
    ///
    /// `f` must not, directly or transitively, invoke an allocator entry point
    /// on the current thread.
    unsafe fn with_allocator_unguarded_for_policy<P, R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R>
    where
        P: mnemosyne_core::AllocPolicy + PolicySlotSelection<B>;

    /// Returns the initialized mode-keyed allocator pointer, arming its TLS
    /// slot when necessary.
    fn get_allocator_ptr_for_policy<P>() -> *mut core::ffi::c_void
    where
        P: mnemosyne_core::AllocPolicy + PolicySlotSelection<B>;

    /// Returns the initialized mode-keyed allocator pointer without creating
    /// its slot.
    fn get_allocator_ptr_raw_for_policy<P>() -> *mut core::ffi::c_void
    where
        P: mnemosyne_core::AllocPolicy + PolicySlotSelection<B>;

    /// Returns the raw pointer for a statically selected free-list encoding
    /// mode without creating its slot.
    fn get_allocator_ptr_raw_for_encryption<const ENCRYPTED: bool>() -> *mut core::ffi::c_void;
}
