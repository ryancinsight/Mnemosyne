//! Platform-specific OS TLS key allocation and thread-local pointer storage.
//!
//! All items are gated behind `not(nightly_tls_active) + not(std_tls) + not(miri)`
//! and accessed from the parent `tls` module as `os_key::*`.

use core::sync::atomic::Ordering;
pub(super) static PROFILER_TLS_KEY: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(u32::MAX);

/// SSOT for the publish-or-free CAS used by both Windows and POSIX key init.
#[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), not(miri)))]
#[inline(always)]
fn cas_tls_key(
    atomic_key: &core::sync::atomic::AtomicU32,
    key: u32,
    free_fn: impl FnOnce(u32),
) -> Option<u32> {
    match atomic_key.compare_exchange(u32::MAX, key, Ordering::Relaxed, Ordering::Relaxed) {
        Ok(_) => Some(key),
        Err(existing) => {
            free_fn(key);
            (existing != u32::MAX).then_some(existing)
        }
    }
}

#[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), not(miri)))]
#[inline(always)]
pub(super) fn get_os_tls_key(atomic_key: &core::sync::atomic::AtomicU32) -> Option<u32> {
    // The atomic publishes an immutable OS TLS slot index only. It does not
    // protect any Rust memory dependency, so relaxed ordering is sufficient.
    let mut key = atomic_key.load(Ordering::Relaxed);
    if key == u32::MAX {
        key = init_os_tls_key(atomic_key)?;
    }
    Some(key)
}

#[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), not(miri)))]
#[cold]
#[inline(never)]
fn init_os_tls_key(atomic_key: &core::sync::atomic::AtomicU32) -> Option<u32> {
    // SAFETY: each branch calls the platform TLS-key FFI with valid arguments —
    // `TlsAlloc`/`TlsFree` take no pointers, `pthread_key_create` receives a
    // valid `&mut key` out-param and a `None` destructor, and any key passed to
    // `TlsFree`/`pthread_key_delete` was just allocated by this call. On a lost
    // publication CAS the freshly-allocated key is freed exactly once.
    unsafe {
        #[cfg(windows)]
        {
            unsafe extern "system" {
                fn TlsAlloc() -> u32;
                fn TlsFree(dwTlsIndex: u32) -> i32;
            }
            let key = TlsAlloc();
            if key == u32::MAX {
                return None;
            }
            cas_tls_key(atomic_key, key, |k| {
                TlsFree(k);
            })
        }
        #[cfg(not(windows))]
        {
            unsafe extern "C" {
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
            cas_tls_key(atomic_key, key, |k| {
                pthread_key_delete(k);
            })
        }
    }
}

// x86_64 Windows reads and writes the slot inline through the TEB
// (`get_profiler_state`); every other target routes through the
// platform getter/setter, so the pair exists exactly there.
#[cfg(all(
    not(nightly_tls_active),
    not(feature = "std_tls"),
    not(miri),
    not(all(windows, target_arch = "x86_64"))
))]
#[inline(always)]
pub(super) fn get_os_tls_value(key: u32) -> *mut core::ffi::c_void {
    // SAFETY: `key` was returned by a successful `get_os_tls_key`, so it is a
    // valid allocated TLS slot index; the platform getter reads this thread's
    // own slot and returns null for an unset slot — it never dereferences `key`.
    unsafe {
        #[cfg(windows)]
        {
            unsafe extern "system" {
                fn TlsGetValue(dwTlsIndex: u32) -> *mut core::ffi::c_void;
            }
            TlsGetValue(key)
        }
        #[cfg(not(windows))]
        {
            unsafe extern "C" {
                fn pthread_getspecific(key: u32) -> *mut core::ffi::c_void;
            }
            pthread_getspecific(key)
        }
    }
}

// Same target partition as `get_os_tls_value`.
#[cfg(all(
    not(nightly_tls_active),
    not(feature = "std_tls"),
    not(miri),
    not(all(windows, target_arch = "x86_64"))
))]
#[inline(always)]
pub(super) fn set_os_tls_value(key: u32, value: *mut core::ffi::c_void) {
    // SAFETY: `key` is a valid allocated TLS slot index; the platform setter
    // stores the opaque `value` in this thread's own slot without dereferencing
    // it.
    unsafe {
        #[cfg(windows)]
        {
            unsafe extern "system" {
                fn TlsSetValue(dwTlsIndex: u32, lpTlsValue: *mut core::ffi::c_void) -> i32;
            }
            TlsSetValue(key, value);
        }
        #[cfg(not(windows))]
        {
            unsafe extern "C" {
                fn pthread_setspecific(key: u32, value: *const core::ffi::c_void) -> i32;
            }
            pthread_setspecific(key, value);
        }
    }
}

/// Reads the Thread Environment Block (TEB) self-pointer from `gs:[0x30]`.
///
/// SSOT for the NtCurrentTeb() expansion-slot paths in `get_teb_tls_slot`
/// and `set_teb_tls_slot`, eliminating two copies of the same inline asm.
///
/// # Safety
/// Only valid on Windows x86-64 outside Miri.
#[cfg(all(
    not(nightly_tls_active),
    not(feature = "std_tls"),
    all(windows, target_arch = "x86_64"),
    not(miri)
))]
#[inline(always)]
unsafe fn read_teb_self() -> *mut u8 {
    let teb: *mut u8;
    // SAFETY: `gs:[0x30]` is the TEB self-pointer in the Windows x86-64 TEB
    // layout; a single aligned read with no side effects.
    unsafe {
        core::arch::asm!(
            "mov {}, gs:[0x30]",
            out(reg) teb,
            options(nostack, preserves_flags, readonly)
        );
    }
    teb
}

/// SSOT for reading `TEB + 0x1780` (`TlsExpansionSlots`) on Windows x86-64.
///
/// # Safety
/// Only valid on Windows x86-64 outside Miri.
#[cfg(all(
    not(nightly_tls_active),
    not(feature = "std_tls"),
    all(windows, target_arch = "x86_64"),
    not(miri)
))]
#[inline(always)]
unsafe fn teb_expansion_slots() -> *mut *mut core::ffi::c_void {
    // SAFETY: `read_teb_self` yields the current thread's TEB; `0x1780` is the
    // fixed x86-64 offset of the `TlsExpansionSlots` pointer in that layout.
    unsafe { *(read_teb_self().add(0x1780) as *mut *mut *mut core::ffi::c_void) }
}

#[cfg(all(
    not(nightly_tls_active),
    not(feature = "std_tls"),
    all(windows, target_arch = "x86_64"),
    not(miri)
))]
/// Reads the value stored in this thread's TEB TLS slot `index`.
///
/// # Safety
///
/// `index` must be a TLS slot index obtained from `TlsAlloc` (so the slot is
/// reserved for this process). The caller relies on the Windows x86-64 TEB
/// layout documented inline below.
#[inline(always)]
pub(super) unsafe fn get_teb_tls_slot(index: u32) -> *mut core::ffi::c_void {
    if index < 64 {
        let val: *mut core::ffi::c_void;
        // SAFETY: on Windows x86-64 the `gs` segment base is the current
        // thread's TEB, and `gs:[0x1480 + index*8]` indexes the TEB's fixed
        // `TlsSlots[64]` array (offset 0x1480 on x64). For `index < 64` this is
        // a single aligned load of this thread's own slot — always-mapped
        // thread-local OS storage, no side effects (`nostack`, `readonly`).
        unsafe {
            core::arch::asm!(
                "mov {}, gs:[0x1480 + {} * 8]",
                out(reg) val,
                in(reg) index as usize,
                options(nostack, preserves_flags, readonly)
            );
        }
        val
    } else {
        // SAFETY: reads `TEB + 0x1780` via the dedicated SSOT helper.
        let expansion_slots = unsafe { teb_expansion_slots() };
        if expansion_slots.is_null() {
            core::ptr::null_mut()
        } else {
            // SAFETY: the expansion array is non-null (just checked) and was
            // sized to cover every allocated index >= 64, so `index - 64` is in
            // bounds for a slot reserved by `TlsAlloc`.
            unsafe { *expansion_slots.add(index as usize - 64) }
        }
    }
}

#[cfg(all(
    not(nightly_tls_active),
    not(feature = "std_tls"),
    all(windows, target_arch = "x86_64"),
    not(miri)
))]
/// Stores `value` in this thread's TEB TLS slot `index`.
///
/// # Safety
///
/// `index` must be a TLS slot index obtained from `TlsAlloc`. The caller relies
/// on the Windows x86-64 TEB layout documented inline below.
#[inline(always)]
pub(super) unsafe fn set_teb_tls_slot(index: u32, value: *mut core::ffi::c_void) {
    if index < 64 {
        // SAFETY: `gs:[0x1480 + index*8]` is this thread's own `TlsSlots[index]`
        // entry (TEB `TlsSlots[64]` array, fixed x64 offset 0x1480); a single
        // aligned store to always-mapped thread-local OS storage.
        unsafe {
            core::arch::asm!(
                "mov gs:[0x1480 + {} * 8], {}",
                in(reg) index as usize,
                in(reg) value,
                options(nostack, preserves_flags)
            );
        }
    } else {
        // SAFETY: reads `TEB + 0x1780` via the dedicated SSOT helper.
        let expansion_slots = unsafe { teb_expansion_slots() };
        if !expansion_slots.is_null() {
            // SAFETY: the array is non-null (just checked) and covers every
            // allocated index >= 64, so `index - 64` is an in-bounds slot.
            unsafe { *expansion_slots.add(index as usize - 64) = value };
        }
    }
}
