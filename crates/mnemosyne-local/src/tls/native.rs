//! Native OS TLS providers using platform APIs and x86_64 TEB ASM.
//!
//! `NativeOsTls` delegates to `TlsGetValue`/`pthread_getspecific`.
//! `AsmTls` uses direct TEB array indexing via inline ASM on Windows x86_64
//! and falls back to `NativeOsTls` on other architectures.

use super::os_helpers::{get_os_tls_key, get_os_tls_value, set_os_tls_value};
#[cfg(all(windows, target_arch = "x86_64", not(miri)))]
use super::os_helpers::{get_teb_tls_slot, set_teb_tls_slot};
use super::traits::{TlsProvider, TlsSlotAccess};
use crate::ThreadAllocator;
use mnemosyne_arena::HasSegmentPool;

/// Platform-native TLS provider using OS-level slots (`TlsGetValue` / `pthread_getspecific`).
pub struct NativeOsTls<B, S>(core::marker::PhantomData<(B, S)>);

impl<B: HasSegmentPool, S: TlsSlotAccess<B>> TlsProvider<B> for NativeOsTls<B, S> {
    const IDENTIFIER: &'static str = "NativeOsTls";

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        let Some(key) = get_os_tls_key(S::get_os_tls_key()) else {
            return S::get_slot_standard(|slot| {
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            });
        };
        let ptr = get_os_tls_value(key);
        if !ptr.is_null() {
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            alloc.is_allocating = true;
            let result = f(alloc);
            alloc.is_allocating = false;
            Some(result)
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                set_os_tls_value(key, alloc_ptr);
                slot.os_key.set(key);
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            })
        }
    }

    #[inline(always)]
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        <NativeOsTls<B, S> as TlsProvider<B>>::with_allocator(f)
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        let Some(key) = get_os_tls_key(S::get_os_tls_key()) else {
            return S::get_slot_standard(|slot| {
                S::arm_thread_exit(slot);
                unsafe { slot.with_allocator_unguarded(f) }
            });
        };
        let ptr = get_os_tls_value(key);
        if !ptr.is_null() {
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            Some(f(alloc))
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                set_os_tls_value(key, alloc_ptr);
                slot.os_key.set(key);
                S::arm_thread_exit(slot);
                unsafe { slot.with_allocator_unguarded(f) }
            })
        }
    }

    #[inline(always)]
    fn get_allocator_ptr() -> *mut core::ffi::c_void {
        let Some(key) = get_os_tls_key(S::get_os_tls_key()) else {
            return S::get_slot_standard(|slot| slot.allocator_ptr());
        };
        let ptr = get_os_tls_value(key);
        if !ptr.is_null() {
            ptr
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                set_os_tls_value(key, alloc_ptr);
                slot.os_key.set(key);
                alloc_ptr
            })
        }
    }

    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        get_os_tls_key(S::get_os_tls_key()).map_or(core::ptr::null_mut(), get_os_tls_value)
    }
}

/// Ultra-low latency TLS provider using direct TEB array indexing via inline assembly on Windows x86_64.
///
/// Falls back to `NativeOsTls` on other architectures.
pub struct AsmTls<B, S>(core::marker::PhantomData<(B, S)>);

#[cfg(all(windows, target_arch = "x86_64", not(miri)))]
impl<B: HasSegmentPool, S: TlsSlotAccess<B>> TlsProvider<B> for AsmTls<B, S> {
    const IDENTIFIER: &'static str = "AsmTls";

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        let Some(key) = get_os_tls_key(S::get_os_tls_key()) else {
            return S::get_slot_standard(|slot| {
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            });
        };
        let ptr = unsafe { get_teb_tls_slot(key) };
        if !ptr.is_null() {
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            alloc.is_allocating = true;
            let result = f(alloc);
            alloc.is_allocating = false;
            Some(result)
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                unsafe { set_teb_tls_slot(key, alloc_ptr) };
                slot.os_key.set(key);
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            })
        }
    }

    #[inline(always)]
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        <AsmTls<B, S> as TlsProvider<B>>::with_allocator(f)
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        let Some(key) = get_os_tls_key(S::get_os_tls_key()) else {
            return S::get_slot_standard(|slot| {
                S::arm_thread_exit(slot);
                unsafe { slot.with_allocator_unguarded(f) }
            });
        };
        let ptr = unsafe { get_teb_tls_slot(key) };
        if !ptr.is_null() {
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            Some(f(alloc))
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                unsafe { set_teb_tls_slot(key, alloc_ptr) };
                slot.os_key.set(key);
                S::arm_thread_exit(slot);
                unsafe { slot.with_allocator_unguarded(f) }
            })
        }
    }

    #[inline(always)]
    fn get_allocator_ptr() -> *mut core::ffi::c_void {
        let Some(key) = get_os_tls_key(S::get_os_tls_key()) else {
            return S::get_slot_standard(|slot| slot.allocator_ptr());
        };
        let ptr = unsafe { get_teb_tls_slot(key) };
        if !ptr.is_null() {
            ptr
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                unsafe { set_teb_tls_slot(key, alloc_ptr) };
                slot.os_key.set(key);
                alloc_ptr
            })
        }
    }

    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        get_os_tls_key(S::get_os_tls_key()).map_or(core::ptr::null_mut(), |key| unsafe {
            get_teb_tls_slot(key)
        })
    }
}

#[cfg(any(not(all(windows, target_arch = "x86_64")), miri))]
impl<B: HasSegmentPool, S: TlsSlotAccess<B>> TlsProvider<B> for AsmTls<B, S> {
    const IDENTIFIER: &'static str = "AsmTls (Fallback)";

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        <NativeOsTls<B, S> as TlsProvider<B>>::with_allocator(f)
    }

    #[inline(always)]
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        <NativeOsTls<B, S> as TlsProvider<B>>::with_allocator_guard(f)
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        unsafe { <NativeOsTls<B, S> as TlsProvider<B>>::with_allocator_unguarded(f) }
    }

    #[inline(always)]
    fn get_allocator_ptr() -> *mut core::ffi::c_void {
        <NativeOsTls<B, S> as TlsProvider<B>>::get_allocator_ptr()
    }

    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        <NativeOsTls<B, S> as TlsProvider<B>>::get_allocator_ptr_raw()
    }
}
