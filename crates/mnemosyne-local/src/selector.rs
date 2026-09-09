//! The thread-local cache selector macro and its per-backend
//! instantiations.

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
                    // SAFETY: `QUICK_ALLOCATOR_PTR` is `#[thread_local]`, so this
                    // reads the calling thread's own instance and no other thread
                    // can observe or race it. The read copies a pointer value and
                    // creates no reference, so it cannot alias a live borrow.
                    unsafe { QUICK_ALLOCATOR_PTR }
                }

                #[cfg(nightly_tls_active)]
                #[inline(always)]
                fn set_quick_allocator_ptr(ptr: *mut core::ffi::c_void) {
                    // SAFETY: as above -- a `#[thread_local]` static written by
                    // its owning thread. The write stores a pointer value and
                    // takes no reference, so no borrow of the static is live
                    // across it.
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
                    // SAFETY: `ENCRYPTED_QUICK_ALLOCATOR_PTR` is `#[thread_local]`,
                    // so this reads the calling thread's own instance. The read
                    // copies a pointer value and creates no reference.
                    unsafe { ENCRYPTED_QUICK_ALLOCATOR_PTR }
                }

                #[cfg(nightly_tls_active)]
                #[inline(always)]
                fn set_quick_allocator_ptr(ptr: *mut core::ffi::c_void) {
                    // SAFETY: as above -- a `#[thread_local]` static written by
                    // its owning thread, storing a pointer value with no
                    // reference taken.
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

            impl $crate::tls_slot::PolicySlotSelection<$backend>
                for mnemosyne_core::policy::StandardPolicy
            {
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
                    $crate::internal::ensure_options_initialized();
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr()
                }

                #[inline(always)]
                fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr_raw()
                }
            }

            impl $crate::tls_slot::PolicySlotSelection<$backend>
                for mnemosyne_core::policy::SecurePolicy
            {
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
                    $crate::internal::ensure_options_initialized();
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr()
                }

                #[inline(always)]
                fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr_raw()
                }
            }

            impl $crate::tls_slot::PolicySlotSelection<$backend>
                for mnemosyne_core::policy::HardenedPolicy
            {
                #[inline(always)]
                fn with_allocator<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    <EncryptedSelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator(f)
                }

                #[inline(always)]
                unsafe fn with_allocator_unguarded<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    unsafe { <EncryptedSelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator_unguarded(f) }
                }

                #[inline(always)]
                fn get_allocator_ptr() -> *mut core::ffi::c_void {
                    $crate::internal::ensure_options_initialized();
                    <EncryptedSelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr()
                }

                #[inline(always)]
                fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
                    <EncryptedSelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr_raw()
                }
            }

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
                    // SAFETY: this function is itself `unsafe`, and its contract
                    // is the provider's: `f` must not re-enter the allocator.
                    // The obligation is forwarded to the caller unchanged.
                    unsafe { <SelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator_unguarded(f) }
                }

                #[inline(always)]
                fn get_allocator_ptr() -> *mut core::ffi::c_void {
                    $crate::internal::ensure_options_initialized();
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr()
                }

                #[inline(always)]
                fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr_raw()
                }

                #[inline(always)]
                fn register_current_allocator_ptr(ptr: *mut core::ffi::c_void) {
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::register_current_allocator_ptr(ptr);
                    <EncryptedSelectedTls as $crate::tls::TlsProvider<$backend>>::register_current_allocator_ptr(ptr);
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
                        // SAFETY: this function is itself `unsafe` and carries the
                        // provider's contract -- `f` must not re-enter the
                        // allocator -- which is forwarded unchanged. The branch
                        // only selects which provider owns the thread's cache.
                        unsafe { <EncryptedSelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator_unguarded(f) }
                    } else {
                        // SAFETY: as in the encrypted branch above.
                        unsafe { <SelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator_unguarded(f) }
                    }
                }

                #[inline(always)]
                fn get_allocator_ptr_for_policy<P: mnemosyne_core::AllocPolicy>() -> *mut core::ffi::c_void {
                    $crate::internal::ensure_options_initialized();
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
