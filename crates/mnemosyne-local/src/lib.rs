//! Thread-local cache allocation and deallocation routing.

#![no_std]
// The `nightly_tls` feature requests the ELF/PE `#[thread_local]` accessor.
// The unstable path is compiled only when the active compiler is nightly;
// stable builds, including `--all-features`, use the portable TLS provider.
#![cfg_attr(nightly_tls_active, feature(thread_local))]

extern crate std;

pub mod local_alloc;
pub mod per_cpu;
pub mod tls;

// Phase 4 instrumentation probe. Opt-in via the `dealloc-probe`
// Cargo feature; production builds compile the module out and pay
// zero cost in `thread_free`.
#[cfg(feature = "dealloc-probe")]
pub mod dealloc_counters;

mod alloc;
mod free;
mod options;
mod realloc;
mod tls_slot;
mod usable_size;
mod validation;

#[cfg(test)]
mod tests;

pub use alloc::{thread_alloc, thread_alloc_layout};
pub use free::{thread_free, thread_free_layout};
pub use local_alloc::{SizeClassOccupancy, ThreadAllocator, ThreadAllocatorStats};
pub use options::{
    ensure_options_initialized, mark_options_initialized, reset_options_for_testing,
};
pub use realloc::thread_realloc;
pub use tls_slot::{LocalAllocatorSelector, LocalAllocatorSlot};
#[cfg(nightly_tls_active)]
pub use tls_slot::{ThreadExitReclaim, arm_thread_exit};
pub use usable_size::{thread_allocator_stats, usable_size};

// Re-export internal details used by the macros/internal paths
#[doc(hidden)]
pub use free::do_local_free_internal;
#[doc(hidden)]
pub use realloc::small_realloc_fits_existing_class;
#[doc(hidden)]
pub use validation::{initialize_allocated_bytes, poison_freed_bytes};

#[doc(hidden)]
pub mod internal {
    pub use crate::ThreadAllocator;
    pub use crate::ensure_options_initialized;
    pub use crate::{
        do_local_free_internal, initialize_allocated_bytes, poison_freed_bytes,
        small_realloc_fits_existing_class, thread_free_layout,
    };
    pub use core::alloc::Layout;
    pub use core::ptr::NonNull;
    pub use mnemosyne_arena::HasSegmentPool;
    pub use mnemosyne_arena::{allocate_large_or_huge, deallocate_large_or_huge};
    pub use mnemosyne_core::constants::{
        MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, PAGE_SHIFT, PAGES_PER_SEGMENT, SEGMENT_SIZE,
    };
    pub use mnemosyne_core::size_class::size_to_class_nonzero;
    pub use mnemosyne_core::types::{Block, Page, Segment};
    pub use mnemosyne_core::validation::is_valid_layout_alloc_request;
}

/// Helper macro to generate zero-cost backend-specific thread-local cache pools.
#[macro_export]
macro_rules! impl_local_allocator_selector {
    ($backend:ty) => {
        const _: () = {
            // Under nightly `nightly_tls`, declare ALLOCATOR_SLOT with #[thread_local].
            #[cfg(nightly_tls_active)]
            #[thread_local]
            static ALLOCATOR_SLOT: $crate::LocalAllocatorSlot<$backend> =
                $crate::LocalAllocatorSlot::new();

            // Under stable or non-nightly_tls, declare ALLOCATOR_SLOT via std::thread_local!.
            #[cfg(not(nightly_tls_active))]
            std::thread_local! {
                static ALLOCATOR_SLOT: $crate::LocalAllocatorSlot<$backend> = const {
                    $crate::LocalAllocatorSlot::new()
                };
            }

            // Expose the slot access cells/guards needed by our TLS strategies.
            std::thread_local! {
                static CACHED_SLOT_PTR: core::cell::Cell<*mut core::ffi::c_void> = const {
                    core::cell::Cell::new(core::ptr::null_mut())
                };

                #[cfg(nightly_tls_active)]
                static ALLOCATOR_EXIT_GUARD: $crate::ThreadExitReclaim<$backend> = const {
                    $crate::ThreadExitReclaim::new()
                };
            }

            #[cfg(nightly_tls_active)]
            #[thread_local]
            static mut QUICK_ALLOCATOR_PTR: *mut core::ffi::c_void = core::ptr::null_mut();

            static OS_TLS_KEY: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(u32::MAX);

            struct SlotAccess;
            impl $crate::tls::TlsSlotAccess<$backend> for SlotAccess {
                #[inline(always)]
                fn get_slot_standard<R>(f: impl FnOnce(&$crate::LocalAllocatorSlot<$backend>) -> R) -> R {
                    #[cfg(nightly_tls_active)]
                    {
                        // In nightly `nightly_tls`, get_slot_standard falls back to the static reference.
                        f(&ALLOCATOR_SLOT)
                    }
                    #[cfg(not(nightly_tls_active))]
                    {
                        ALLOCATOR_SLOT.with(f)
                    }
                }

                #[inline(always)]
                fn get_cached_cell<R>(f: impl FnOnce(&core::cell::Cell<*mut core::ffi::c_void>) -> R) -> R {
                    CACHED_SLOT_PTR.with(f)
                }

                #[inline(always)]
                fn arm_thread_exit(slot: &$crate::LocalAllocatorSlot<$backend>) {
                    #[cfg(nightly_tls_active)]
                    {
                        $crate::arm_thread_exit(slot, &ALLOCATOR_EXIT_GUARD);
                    }
                    #[cfg(not(nightly_tls_active))]
                    {
                        // No-op for stable path: LocalAllocatorSlot is registered automatically by standard thread_local!.
                        let _ = slot;
                    }
                }

                #[inline(always)]
                fn get_os_tls_key() -> &'static core::sync::atomic::AtomicU32 {
                    &OS_TLS_KEY
                }

                #[cfg(nightly_tls_active)]
                #[inline(always)]
                fn get_slot_nightly<R>(f: impl FnOnce(&$crate::LocalAllocatorSlot<$backend>) -> R) -> R {
                    f(&ALLOCATOR_SLOT)
                }

                #[cfg(nightly_tls_active)]
                #[inline(always)]
                fn get_quick_allocator_ptr() -> *mut core::ffi::c_void {
                    unsafe { QUICK_ALLOCATOR_PTR }
                }

                #[cfg(nightly_tls_active)]
                #[inline(always)]
                fn set_quick_allocator_ptr(ptr: *mut core::ffi::c_void) {
                    unsafe { QUICK_ALLOCATOR_PTR = ptr; }
                }
            }

            // Statically select the best TLS provider based on compile target and features.
            #[cfg(all(nightly_tls_active, not(miri)))]
            type SelectedTls = $crate::tls::NightlyTls<$backend, SlotAccess>;

            #[cfg(any(
                miri,
                all(not(nightly_tls_active), feature = "std_tls")
            ))]
            type SelectedTls = $crate::tls::CachedCellTls<$backend, SlotAccess>;

            #[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), all(windows, target_arch = "x86_64"), not(miri)))]
            type SelectedTls = $crate::tls::AsmTls<$backend, SlotAccess>;

            #[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), any(not(all(windows, target_arch = "x86_64")), miri)))]
            type SelectedTls = $crate::tls::NativeOsTls<$backend, SlotAccess>;

            // The hardened policy gets a distinct cache so pages owned by the
            // standard and encrypted policies cannot share one allocator's
            // active-page lists. The two slots intentionally use the same TLS
            // provider shape; only the slot identity changes.
            #[cfg(nightly_tls_active)]
            #[thread_local]
            static ENCRYPTED_ALLOCATOR_SLOT: $crate::LocalAllocatorSlot<$backend> =
                $crate::LocalAllocatorSlot::new();

            #[cfg(not(nightly_tls_active))]
            std::thread_local! {
                static ENCRYPTED_ALLOCATOR_SLOT: $crate::LocalAllocatorSlot<$backend> = const {
                    $crate::LocalAllocatorSlot::new()
                };
            }

            std::thread_local! {
                static ENCRYPTED_CACHED_SLOT_PTR: core::cell::Cell<*mut core::ffi::c_void> = const {
                    core::cell::Cell::new(core::ptr::null_mut())
                };

                #[cfg(nightly_tls_active)]
                static ENCRYPTED_ALLOCATOR_EXIT_GUARD: $crate::ThreadExitReclaim<$backend> = const {
                    $crate::ThreadExitReclaim::new()
                };
            }

            #[cfg(nightly_tls_active)]
            #[thread_local]
            static mut ENCRYPTED_QUICK_ALLOCATOR_PTR: *mut core::ffi::c_void = core::ptr::null_mut();

            static ENCRYPTED_OS_TLS_KEY: core::sync::atomic::AtomicU32 =
                core::sync::atomic::AtomicU32::new(u32::MAX);

            struct EncryptedSlotAccess;
            impl $crate::tls::TlsSlotAccess<$backend> for EncryptedSlotAccess {
                #[inline(always)]
                fn get_slot_standard<R>(
                    f: impl FnOnce(&$crate::LocalAllocatorSlot<$backend>) -> R,
                ) -> R {
                    #[cfg(nightly_tls_active)]
                    {
                        f(&ENCRYPTED_ALLOCATOR_SLOT)
                    }
                    #[cfg(not(nightly_tls_active))]
                    {
                        ENCRYPTED_ALLOCATOR_SLOT.with(f)
                    }
                }

                #[inline(always)]
                fn get_cached_cell<R>(
                    f: impl FnOnce(&core::cell::Cell<*mut core::ffi::c_void>) -> R,
                ) -> R {
                    ENCRYPTED_CACHED_SLOT_PTR.with(f)
                }

                #[inline(always)]
                fn arm_thread_exit(slot: &$crate::LocalAllocatorSlot<$backend>) {
                    #[cfg(nightly_tls_active)]
                    {
                        $crate::arm_thread_exit(slot, &ENCRYPTED_ALLOCATOR_EXIT_GUARD);
                    }
                    #[cfg(not(nightly_tls_active))]
                    {
                        let _ = slot;
                    }
                }

                #[inline(always)]
                fn get_os_tls_key() -> &'static core::sync::atomic::AtomicU32 {
                    &ENCRYPTED_OS_TLS_KEY
                }

                #[cfg(nightly_tls_active)]
                #[inline(always)]
                fn get_slot_nightly<R>(
                    f: impl FnOnce(&$crate::LocalAllocatorSlot<$backend>) -> R,
                ) -> R {
                    f(&ENCRYPTED_ALLOCATOR_SLOT)
                }

                #[cfg(nightly_tls_active)]
                #[inline(always)]
                fn get_quick_allocator_ptr() -> *mut core::ffi::c_void {
                    unsafe { ENCRYPTED_QUICK_ALLOCATOR_PTR }
                }

                #[cfg(nightly_tls_active)]
                #[inline(always)]
                fn set_quick_allocator_ptr(ptr: *mut core::ffi::c_void) {
                    unsafe { ENCRYPTED_QUICK_ALLOCATOR_PTR = ptr; }
                }
            }

            #[cfg(all(nightly_tls_active, not(miri)))]
            type EncryptedSelectedTls = $crate::tls::NightlyTls<$backend, EncryptedSlotAccess>;

            #[cfg(any(
                miri,
                all(not(nightly_tls_active), feature = "std_tls")
            ))]
            type EncryptedSelectedTls =
                $crate::tls::CachedCellTls<$backend, EncryptedSlotAccess>;

            #[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), all(windows, target_arch = "x86_64"), not(miri)))]
            type EncryptedSelectedTls = $crate::tls::AsmTls<$backend, EncryptedSlotAccess>;

            #[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), any(not(all(windows, target_arch = "x86_64")), miri)))]
            type EncryptedSelectedTls =
                $crate::tls::NativeOsTls<$backend, EncryptedSlotAccess>;

            impl $crate::LocalAllocatorSelector<$backend> for $backend {
                #[inline(always)]
                fn with_allocator<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator(f)
                }

                #[inline(always)]
                unsafe fn with_allocator_unguarded<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    unsafe { <SelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator_unguarded(f) }
                }

                #[inline(always)]
                fn get_allocator_ptr() -> *mut core::ffi::c_void {
                    $crate::ensure_options_initialized();
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr()
                }

                #[inline(always)]
                fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr_raw()
                }

                #[inline(always)]
                fn with_allocator_for_policy<P: mnemosyne_core::AllocPolicy, R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    if P::ENABLE_FREE_LIST_ENCRYPTION {
                        <EncryptedSelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator(f)
                    } else {
                        <SelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator(f)
                    }
                }

                #[inline(always)]
                unsafe fn with_allocator_unguarded_for_policy<P: mnemosyne_core::AllocPolicy, R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    if P::ENABLE_FREE_LIST_ENCRYPTION {
                        unsafe { <EncryptedSelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator_unguarded(f) }
                    } else {
                        unsafe { <SelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator_unguarded(f) }
                    }
                }

                #[inline(always)]
                fn get_allocator_ptr_for_policy<P: mnemosyne_core::AllocPolicy>() -> *mut core::ffi::c_void {
                    $crate::ensure_options_initialized();
                    if P::ENABLE_FREE_LIST_ENCRYPTION {
                        <EncryptedSelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr()
                    } else {
                        <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr()
                    }
                }

                #[inline(always)]
                fn get_allocator_ptr_raw_for_policy<P: mnemosyne_core::AllocPolicy>() -> *mut core::ffi::c_void {
                    if P::ENABLE_FREE_LIST_ENCRYPTION {
                        <EncryptedSelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr_raw()
                    } else {
                        <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr_raw()
                    }
                }

                #[inline(always)]
                fn get_allocator_ptr_raw_for_encryption<const ENCRYPTED: bool>() -> *mut core::ffi::c_void {
                    if ENCRYPTED {
                        <EncryptedSelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr_raw()
                    } else {
                        <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr_raw()
                    }
                }
            }
        };
    };
}

impl_local_allocator_selector!(mnemosyne_backend::MemoryBackendWrapper);
impl_local_allocator_selector!(mnemosyne_backend::CudaUnifiedBackend);
impl_local_allocator_selector!(mnemosyne_backend::CudaDeviceBackend);
impl_local_allocator_selector!(mnemosyne_backend::CudaHbmBackend);
impl_local_allocator_selector!(mnemosyne_backend::CudaGddrBackend);
impl_local_allocator_selector!(mnemosyne_backend::CudaHostPinnedBackend);
