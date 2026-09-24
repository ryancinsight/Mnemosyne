//! `TlsSlotAccess` and `TlsProvider` trait definitions.
//!
//! These sealed traits form the monomorphization boundary for thread-local storage
//! access. All TLS provider implementations in sibling leaf modules implement
//! `TlsProvider<B>` against types that implement `TlsSlotAccess<B>`.

use crate::{LocalAllocatorSlot, ThreadAllocator};
use core::sync::atomic::AtomicU32;
use mnemosyne_arena::HasSegmentPool;

/// Trait providing access to the raw thread-local slot and exit registration hook.
pub trait TlsSlotAccess<B: HasSegmentPool>: 'static {
    /// Executes the closure with a reference to the standard thread-local allocator slot.
    fn get_slot_standard<R>(f: impl FnOnce(&LocalAllocatorSlot<B>) -> R) -> R;

    /// Executes the closure with a reference to the thread-local pointer cache cell.
    fn get_cached_cell<R>(f: impl FnOnce(&core::cell::Cell<*mut core::ffi::c_void>) -> R) -> R;

    /// Arms the thread-exit reclamation sentinel for the given slot.
    fn arm_thread_exit(slot: &LocalAllocatorSlot<B>);

    /// Returns the static atomic holding the platform-native OS TLS key.
    fn get_os_tls_key() -> &'static AtomicU32;

    /// Executes the closure with a reference to the nightly `#[thread_local]` static.
    #[cfg(nightly_tls_active)]
    fn get_slot_nightly<R>(f: impl FnOnce(&LocalAllocatorSlot<B>) -> R) -> R;

    /// Returns the raw thread-local pointer to the allocator cache without closure overhead.
    #[cfg(nightly_tls_active)]
    fn get_quick_allocator_ptr() -> *mut core::ffi::c_void;

    /// Sets the raw thread-local pointer to the allocator cache.
    #[cfg(nightly_tls_active)]
    fn set_quick_allocator_ptr(ptr: *mut core::ffi::c_void);

    /// Arms the thread-exit sentinel and runs `f` with guarded allocator access.
    ///
    /// This is the **SSOT** for the repeated pattern used in
    /// `StandardTls::with_allocator`, `NativeOsTls::with_allocator` (fallback),
    /// and `CachedCellTls::with_allocator` (init branch): call
    /// `get_slot_standard`, arm the exit sentinel, then call
    /// `LocalAllocatorSlot::with_allocator` via the slot's own pointer.
    #[inline(always)]
    fn slot_access_armed<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        Self::get_slot_standard(|slot| {
            Self::arm_thread_exit(slot);
            // SAFETY: `allocator_ptr()` returns this slot's own live address;
            // `arm_thread_exit` just registered the exit sentinel.
            unsafe { LocalAllocatorSlot::<B>::with_allocator(slot.allocator_ptr(), f) }
        })
    }

    /// Runs `f` with unguarded allocator access via the standard slot (no re-entrancy arm).
    ///
    /// This is the **SSOT** for the repeated pattern used in
    /// `StandardTls::with_allocator_unguarded`, `NativeOsTls::with_allocator_unguarded`
    /// (fallback), and `CachedCellTls::with_allocator_unguarded` (init branch):
    /// call `get_slot_standard` then `LocalAllocatorSlot::with_allocator_unguarded`
    /// without arming the re-entrancy gate.
    ///
    /// # Safety
    ///
    /// `f` must not re-enter the allocator.
    #[inline(always)]
    unsafe fn slot_access_unguarded<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        Self::get_slot_standard(|slot| {
            // SAFETY: caller upholds the no-re-entry contract; `allocator_ptr`
            // returns this slot's own live address.
            unsafe { LocalAllocatorSlot::<B>::with_allocator_unguarded(slot.allocator_ptr(), f) }
        })
    }
}

/// Monomorphized interface for accessing thread-local allocator caches.
pub trait TlsProvider<B: HasSegmentPool>: 'static {
    /// Friendly identifier for diagnostics and benchmarking.
    const IDENTIFIER: &'static str;

    /// Registers a manually created thread-affine allocator cache as the current
    /// thread's active pointer for this backend mode, so raw `thread_free` /
    /// `realloc` / `usable_size` lookups can recognize direct
    /// `ThreadAllocator::new()` instances in the same way as the TLS-backed
    /// public entry points.
    fn register_current_allocator_ptr(ptr: *mut core::ffi::c_void);

    /// Runs `f` with a mutable reference to the thread-local allocator cache,
    /// arming the re-entrancy guard.
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R>;

    /// Runs `f` with the thread-local allocator cache without arming the re-entrancy guard.
    ///
    /// # Safety
    ///
    /// `f` must not re-enter the allocator.
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R>;

    /// Returns the raw pointer to the thread-local allocator cache.
    fn get_allocator_ptr() -> *mut core::ffi::c_void;

    /// Returns the raw pointer to the thread-local allocator cache without triggering lazy initialization.
    ///
    /// The default delegates to [`Self::get_allocator_ptr`], which is correct
    /// for providers where initialization is implicit on first access (e.g.
    /// `StandardTls`). Providers that use a fast raw-cache path
    /// (e.g. `CachedCellTls`, `NativeOsTls`, `AsmTls`) override this to
    /// return the cached value without triggering the lazy-initialization
    /// branch.
    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        Self::get_allocator_ptr()
    }
}
