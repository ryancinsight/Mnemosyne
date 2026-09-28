//! Platform-native OS thread-local-storage (TLS) slot access.
//!
//! The single home for the raw OS-TLS FFI: `TlsAlloc` / `TlsGetValue` /
//! `TlsSetValue` on Windows and `pthread_key_create` / `pthread_getspecific` /
//! `pthread_setspecific` elsewhere, plus the Windows x86-64 Thread Environment
//! Block (TEB) fast path that reads and writes a slot inline through `gs:`.
//!
//! Both TLS consumers in the workspace depend on this module rather than
//! re-declaring the FFI: `mnemosyne-local`'s native providers and
//! `mnemosyne-prof`'s profiler-state slot. The module carries no policy or
//! feature gates of its own — a caller applies its own `cfg` to its call sites
//! — so the two crates cannot drift into two implementations of the same
//! `unsafe` surface.
//!
//! # Failure policy (single source of truth)
//!
//! A failed OS-slot write is fatal: [`write_value`] and [`write_teb_slot`]
//! route to [`crate::abort::abort_on_corruption`] (abort under `std`, panic
//! otherwise). This adopts `mnemosyne-local`'s original policy over
//! `mnemosyne-prof`'s silent ignore, because a dropped write leaves the slot
//! empty and the next read re-initializes it — quietly publishing a second
//! thread-local object over the first. For the same reason the TEB write keeps
//! `local`'s null-expansion-array fallback to the OS setter instead of
//! `prof`'s silent drop.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};

/// Sentinel meaning "no OS TLS key allocated yet".
const NO_KEY: u32 = u32::MAX;

/// Returns this process's OS TLS key, allocating it once on first use.
///
/// `atomic_key` holds the slot index and is the publish point: the first
/// caller allocates a key and wins a `compare_exchange`; a loser frees the key
/// it just allocated and adopts the winner's. `None` means key allocation
/// failed.
///
/// The atomic publishes an immutable slot index only — it protects no Rust
/// memory — so the relaxed load is sufficient. The publish CAS takes the
/// stronger of the two former orderings (`AcqRel`/`Acquire`): the path is cold
/// and acquiring the winner's index on the loser branch costs nothing.
#[inline(always)]
pub fn get_or_init_key(atomic_key: &AtomicU32) -> Option<u32> {
    let key = atomic_key.load(Ordering::Relaxed);
    if key == NO_KEY {
        return init_key(atomic_key);
    }
    Some(key)
}

/// Publishes `key` into `atomic_key`, freeing it if another caller won.
///
/// SSOT for the publish-or-free `compare_exchange` shared by both the Windows
/// and POSIX allocation branches; only `free_fn` differs.
#[inline(always)]
fn publish_key(atomic_key: &AtomicU32, key: u32, free_fn: impl FnOnce(u32)) -> Option<u32> {
    match atomic_key.compare_exchange(NO_KEY, key, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => Some(key),
        Err(existing) => {
            free_fn(key);
            (existing != NO_KEY).then_some(existing)
        }
    }
}

/// Allocates an OS TLS key and publishes it through `atomic_key`.
#[cold]
#[inline(never)]
fn init_key(atomic_key: &AtomicU32) -> Option<u32> {
    // SAFETY: `TlsAlloc`/`TlsFree` take no caller-supplied pointers;
    // `pthread_key_create` receives a valid `&mut key` out-param and a `None`
    // destructor; any key passed to `TlsFree`/`pthread_key_delete` was
    // allocated by this call and is freed at most once on the CAS-loser path.
    unsafe {
        #[cfg(windows)]
        {
            unsafe extern "system" {
                fn TlsAlloc() -> u32;
                fn TlsFree(dwTlsIndex: u32) -> i32;
            }
            let key = TlsAlloc();
            if key == NO_KEY {
                return None;
            }
            publish_key(atomic_key, key, |k| {
                TlsFree(k);
            })
        }
        #[cfg(not(windows))]
        {
            unsafe extern "C" {
                fn pthread_key_create(
                    key: *mut u32,
                    destructor: Option<unsafe extern "C" fn(*mut c_void)>,
                ) -> i32;
                fn pthread_key_delete(key: u32) -> i32;
            }
            let mut key = 0u32;
            if pthread_key_create(&mut key, None) != 0 {
                return None;
            }
            publish_key(atomic_key, key, |k| {
                pthread_key_delete(k);
            })
        }
    }
}

/// Reads the value stored in this thread's OS TLS slot `key`.
///
/// Returns null for an unset slot, which the OS setter/getter contract
/// permits.
#[inline(always)]
pub fn read_value(key: u32) -> *mut c_void {
    // SAFETY: `key` originates from `get_or_init_key`, i.e. a successful
    // `TlsAlloc`/`pthread_key_create`, so it names a live slot; the getter reads
    // this thread's own slot and never dereferences `key`.
    unsafe {
        #[cfg(windows)]
        {
            unsafe extern "system" {
                fn TlsGetValue(dwTlsIndex: u32) -> *mut c_void;
            }
            TlsGetValue(key)
        }
        #[cfg(not(windows))]
        {
            unsafe extern "C" {
                fn pthread_getspecific(key: u32) -> *mut c_void;
            }
            pthread_getspecific(key)
        }
    }
}

/// Stores `value` in this thread's OS TLS slot `key`.
///
/// Aborts the process if the OS rejects the write (see the module-level
/// failure-policy note); `value` is an opaque pointer the slot merely holds
/// and is never dereferenced here. `key` must be a valid OS TLS slot index as
/// returned by [`get_or_init_key`].
///
/// The pointer is forwarded to the private FFI helper rather than issued to
/// the platform call directly, so this public signature stays free of
/// `clippy::not_unsafe_ptr_arg_deref` (a false positive here: the slot stores
/// the pointer and never dereferences it).
#[inline(always)]
pub fn write_value(key: u32, value: *mut c_void) {
    write_value_ffi(key, value);
}

/// Private FFI body of [`write_value`].
///
/// Non-public so the raw-pointer-taking platform call lives outside the
/// public API surface the deref lint inspects.
#[inline(always)]
fn write_value_ffi(key: u32, value: *mut c_void) {
    // SAFETY: `key` is a live slot from `get_or_init_key` (caller contract, see
    // `write_value`); the setter stores the opaque `value` without
    // dereferencing it.
    unsafe {
        #[cfg(windows)]
        {
            unsafe extern "system" {
                fn TlsSetValue(dwTlsIndex: u32, lpTlsValue: *mut c_void) -> i32;
            }
            if TlsSetValue(key, value) == 0 {
                crate::abort::abort_on_corruption("OS TLS slot write failed (TlsSetValue)");
            }
        }
        #[cfg(not(windows))]
        {
            unsafe extern "C" {
                fn pthread_setspecific(key: u32, value: *const c_void) -> i32;
            }
            if pthread_setspecific(key, value) != 0 {
                crate::abort::abort_on_corruption("OS TLS slot write failed (pthread_setspecific)");
            }
        }
    }
}

/// Reads the Thread Environment Block (TEB) self-pointer from `gs:[0x30]`.
///
/// # Safety
/// Only valid on Windows x86-64 outside Miri.
#[cfg(all(windows, target_arch = "x86_64", not(miri)))]
#[inline(always)]
unsafe fn read_teb_self() -> *mut u8 {
    let teb: *mut u8;
    // SAFETY: `gs:[0x30]` is the TEB `Self` pointer in the well-known Windows
    // x86-64 TEB layout; a single aligned read, no write.
    unsafe {
        core::arch::asm!(
            "mov {}, gs:[0x30]",
            out(reg) teb,
            options(nostack, preserves_flags, readonly)
        );
    }
    teb
}

/// Returns the TEB expansion-slot array pointer, or null if not yet allocated.
///
/// # Safety
/// Only valid on Windows x86-64 outside Miri.
#[cfg(all(windows, target_arch = "x86_64", not(miri)))]
#[inline(always)]
unsafe fn teb_expansion_slots() -> *mut *mut c_void {
    // SAFETY: `read_teb_self` yields this thread's TEB; the expansion-slot
    // pointer sits at TEB+0x1780 in the well-known x86-64 layout.
    unsafe { *(read_teb_self().add(0x1780) as *mut *mut *mut c_void) }
}

/// Reads this thread's TEB TLS slot `index` (Windows x86-64 fast path).
///
/// For `index < 64` uses the inline `TlsSlots` array at `gs:[0x1480]`; for
/// larger indices uses the expansion-slot array at `gs:[0x30] + 0x1780`.
///
/// # Safety
/// `index` must be a key returned by `TlsAlloc`.
#[cfg(all(windows, target_arch = "x86_64", not(miri)))]
#[inline(always)]
pub unsafe fn read_teb_slot(index: u32) -> *mut c_void {
    // SAFETY: `index` is a valid `TlsAlloc`-allocated key (caller contract); the
    // inline array (GS+0x1480) and the expansion pointer (GS+0x1780) are within
    // the well-known x86-64 TEB layout. The inline-asm reads only.
    unsafe {
        if index < 64 {
            let val: *mut c_void;
            core::arch::asm!(
                "mov {}, gs:[0x1480 + {} * 8]",
                out(reg) val,
                in(reg) index as usize,
                options(nostack, preserves_flags, readonly)
            );
            val
        } else {
            let expansion_slots = teb_expansion_slots();
            if expansion_slots.is_null() {
                core::ptr::null_mut()
            } else {
                *expansion_slots.add(index as usize - 64)
            }
        }
    }
}

/// Writes this thread's TEB TLS slot `index` (Windows x86-64 fast path).
///
/// Resolves the slot the same way [`read_teb_slot`] does; when the expansion
/// array has not been allocated yet it falls back to [`write_value`] so a write
/// to a high-numbered slot is never silently dropped.
///
/// # Safety
/// `index` must be a key returned by `TlsAlloc`.
#[cfg(all(windows, target_arch = "x86_64", not(miri)))]
#[inline(always)]
pub unsafe fn write_teb_slot(index: u32, value: *mut c_void) {
    // SAFETY: as `read_teb_slot`; the store is a single aligned write to this
    // thread's own slot in the well-known x86-64 TEB layout.
    unsafe {
        if index < 64 {
            core::arch::asm!(
                "mov gs:[0x1480 + {} * 8], {}",
                in(reg) index as usize,
                in(reg) value,
                options(nostack, preserves_flags)
            );
        } else {
            let expansion_slots = teb_expansion_slots();
            if expansion_slots.is_null() {
                write_value(index, value);
            } else {
                *expansion_slots.add(index as usize - 64) = value;
            }
        }
    }
}
