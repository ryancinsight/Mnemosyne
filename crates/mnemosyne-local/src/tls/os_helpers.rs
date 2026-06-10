//! Platform-native OS TLS helper functions.
//!
//! Provides zero-overhead wrappers for `TlsAlloc`/`TlsFree` on Windows and
//! `pthread_key_create`/`pthread_key_delete` on POSIX, plus direct TEB slot
//! access via inline ASM on Windows x86_64.

use core::sync::atomic::{AtomicU32, Ordering};

/// Retrieves an initialized OS TLS key, initializing lazily on first call.
///
/// Returns `None` only when OS TLS key allocation fails.
#[inline(always)]
pub(crate) fn get_os_tls_key(atomic_key: &AtomicU32) -> Option<u32> {
    let mut key = atomic_key.load(Ordering::Relaxed);
    if key == u32::MAX {
        key = init_os_tls_key(atomic_key)?;
    }
    Some(key)
}

#[cold]
#[inline(never)]
fn init_os_tls_key(atomic_key: &AtomicU32) -> Option<u32> {
    unsafe {
        #[cfg(windows)]
        {
            extern "system" {
                fn TlsAlloc() -> u32;
                fn TlsFree(dwTlsIndex: u32) -> i32;
            }
            let key = TlsAlloc();
            if key == u32::MAX {
                return None;
            }
            match atomic_key.compare_exchange(u32::MAX, key, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => Some(key),
                Err(existing) => {
                    TlsFree(key);
                    (existing != u32::MAX).then_some(existing)
                }
            }
        }
        #[cfg(not(windows))]
        {
            extern "C" {
                fn pthread_key_create(
                    key: *mut u32,
                    destructor: Option<unsafe extern "C" fn(*mut core::ffi::c_void)>,
                ) -> i32;
                fn pthread_key_delete(key: u32) -> i32;
            }
            let mut key = 0u32;
            let res = pthread_key_create(&mut key, None);
            if res != 0 {
                return None;
            }
            match atomic_key.compare_exchange(u32::MAX, key, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => Some(key),
                Err(existing) => {
                    pthread_key_delete(key);
                    (existing != u32::MAX).then_some(existing)
                }
            }
        }
    }
}

/// Reads the value stored in the OS TLS slot identified by `key`.
#[inline(always)]
pub(crate) fn get_os_tls_value(key: u32) -> *mut core::ffi::c_void {
    unsafe {
        #[cfg(windows)]
        {
            extern "system" {
                fn TlsGetValue(dwTlsIndex: u32) -> *mut core::ffi::c_void;
            }
            TlsGetValue(key)
        }
        #[cfg(not(windows))]
        {
            extern "C" {
                fn pthread_getspecific(key: u32) -> *mut core::ffi::c_void;
            }
            pthread_getspecific(key)
        }
    }
}

/// Writes a value into the OS TLS slot identified by `key`.
#[inline(always)]
pub(crate) fn set_os_tls_value(key: u32, value: *mut core::ffi::c_void) {
    unsafe {
        #[cfg(windows)]
        {
            extern "system" {
                fn TlsSetValue(dwTlsIndex: u32, lpTlsValue: *mut core::ffi::c_void) -> i32;
            }
            TlsSetValue(key, value);
        }
        #[cfg(not(windows))]
        {
            extern "C" {
                fn pthread_setspecific(key: u32, value: *const core::ffi::c_void) -> i32;
            }
            pthread_setspecific(key, value);
        }
    }
}

/// Reads a TLS slot directly from the Thread Environment Block (TEB) on Windows x86_64.
///
/// For `index < 64`, uses the inline TLS array at GS:0x1480.
/// For `index >= 64`, uses the expansion slot array at GS:0x30 + 0x1780.
///
/// # Safety
/// `index` must be a valid key returned by `TlsAlloc`.
#[cfg(all(windows, target_arch = "x86_64"))]
#[inline(always)]
pub(crate) unsafe fn get_teb_tls_slot(index: u32) -> *mut core::ffi::c_void {
    if index < 64 {
        let val: *mut core::ffi::c_void;
        core::arch::asm!(
            "mov {}, gs:[0x1480 + {} * 8]",
            out(reg) val,
            in(reg) index as usize,
            options(nostack, preserves_flags, readonly)
        );
        val
    } else {
        let teb: *mut u8;
        core::arch::asm!(
            "mov {}, gs:[0x30]",
            out(reg) teb,
            options(nostack, preserves_flags, readonly)
        );
        let expansion_slots = *(teb.add(0x1780) as *mut *mut *mut core::ffi::c_void);
        if expansion_slots.is_null() {
            core::ptr::null_mut()
        } else {
            *expansion_slots.add(index as usize - 64)
        }
    }
}

/// Writes a TLS slot directly into the Thread Environment Block (TEB) on Windows x86_64.
///
/// # Safety
/// `index` must be a valid key returned by `TlsAlloc`.
#[cfg(all(windows, target_arch = "x86_64"))]
#[inline(always)]
pub(crate) unsafe fn set_teb_tls_slot(index: u32, value: *mut core::ffi::c_void) {
    if index < 64 {
        core::arch::asm!(
            "mov gs:[0x1480 + {} * 8], {}",
            in(reg) index as usize,
            in(reg) value,
            options(nostack, preserves_flags)
        );
    } else {
        let teb: *mut u8;
        core::arch::asm!(
            "mov {}, gs:[0x30]",
            out(reg) teb,
            options(nostack, preserves_flags, readonly)
        );
        let expansion_slots = *(teb.add(0x1780) as *mut *mut *mut core::ffi::c_void);
        if !expansion_slots.is_null() {
            *expansion_slots.add(index as usize - 64) = value;
        }
    }
}
