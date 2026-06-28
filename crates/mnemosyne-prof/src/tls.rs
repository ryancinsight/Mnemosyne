#[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), not(miri)))]
use core::sync::atomic::Ordering;

#[derive(Clone, Copy)]
pub(crate) struct ThreadState {
    pub(crate) bytes_until_sample: isize,
    pub(crate) in_hook: bool,
}

#[inline(always)]
pub(crate) fn sample_debit(size: usize) -> isize {
    match isize::try_from(size) {
        Ok(size) => size,
        Err(_) => isize::MAX,
    }
}

#[cfg(nightly_tls_active)]
#[thread_local]
static mut THREAD_STATE: ThreadState = ThreadState {
    bytes_until_sample: 0,
    in_hook: false,
};

#[cfg(not(nightly_tls_active))]
std::thread_local! {
    static THREAD_STATE: core::cell::UnsafeCell<ThreadState> = const {
        core::cell::UnsafeCell::new(ThreadState {
            bytes_until_sample: 0,
            in_hook: false,
        })
    };
}

#[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), not(miri)))]
static PROFILER_TLS_KEY: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(u32::MAX);

#[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), not(miri)))]
#[inline(always)]
fn get_os_tls_key(atomic_key: &core::sync::atomic::AtomicU32) -> Option<u32> {
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
            extern "system" {
                fn TlsAlloc() -> u32;
                fn TlsFree(dwTlsIndex: u32) -> i32;
            }
            let key = TlsAlloc();
            if key == u32::MAX {
                return None;
            }
            match atomic_key.compare_exchange(u32::MAX, key, Ordering::Relaxed, Ordering::Relaxed) {
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
            match atomic_key.compare_exchange(u32::MAX, key, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => Some(key),
                Err(existing) => {
                    pthread_key_delete(key);
                    (existing != u32::MAX).then_some(existing)
                }
            }
        }
    }
}

#[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), not(miri)))]
#[allow(dead_code)]
#[inline(always)]
fn get_os_tls_value(key: u32) -> *mut core::ffi::c_void {
    // SAFETY: `key` was returned by a successful `get_os_tls_key`, so it is a
    // valid allocated TLS slot index; the platform getter reads this thread's
    // own slot and returns null for an unset slot — it never dereferences `key`.
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

#[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), not(miri)))]
#[allow(dead_code)]
#[inline(always)]
fn set_os_tls_value(key: u32, value: *mut core::ffi::c_void) {
    // SAFETY: `key` is a valid allocated TLS slot index; the platform setter
    // stores the opaque `value` in this thread's own slot without dereferencing
    // it.
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
unsafe fn get_teb_tls_slot(index: u32) -> *mut core::ffi::c_void {
    if index < 64 {
        let val: *mut core::ffi::c_void;
        // SAFETY: on Windows x86-64 the `gs` segment base is the current
        // thread's TEB, and `gs:[0x1480 + index*8]` indexes the TEB's fixed
        // `TlsSlots[64]` array (offset 0x1480 on x64). For `index < 64` this is
        // a single aligned load of this thread's own slot — always-mapped
        // thread-local OS storage, no side effects (`nostack`, `readonly`).
        core::arch::asm!(
            "mov {}, gs:[0x1480 + {} * 8]",
            out(reg) val,
            in(reg) index as usize,
            options(nostack, preserves_flags, readonly)
        );
        val
    } else {
        let teb: *mut u8;
        // SAFETY: `gs:[0x30]` is the TEB self-pointer (`NtCurrentTeb`); a single
        // aligned read of an always-mapped field, no side effects.
        core::arch::asm!(
            "mov {}, gs:[0x30]",
            out(reg) teb,
            options(nostack, preserves_flags, readonly)
        );
        // SAFETY: `TEB + 0x1780` is the `TlsExpansionSlots` pointer field (fixed
        // x64 offset); reading it yields the (possibly null) base of the
        // expansion-slot array for indices >= 64.
        let expansion_slots = *(teb.add(0x1780) as *mut *mut *mut core::ffi::c_void);
        if expansion_slots.is_null() {
            core::ptr::null_mut()
        } else {
            // SAFETY: the expansion array is non-null (just checked) and was
            // sized to cover every allocated index >= 64, so `index - 64` is in
            // bounds for a slot reserved by `TlsAlloc`.
            *expansion_slots.add(index as usize - 64)
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
unsafe fn set_teb_tls_slot(index: u32, value: *mut core::ffi::c_void) {
    if index < 64 {
        // SAFETY: `gs:[0x1480 + index*8]` is this thread's own `TlsSlots[index]`
        // entry (TEB `TlsSlots[64]` array, fixed x64 offset 0x1480); a single
        // aligned store to always-mapped thread-local OS storage.
        core::arch::asm!(
            "mov gs:[0x1480 + {} * 8], {}",
            in(reg) index as usize,
            in(reg) value,
            options(nostack, preserves_flags)
        );
    } else {
        let teb: *mut u8;
        // SAFETY: `gs:[0x30]` is the TEB self-pointer; a single aligned read.
        core::arch::asm!(
            "mov {}, gs:[0x30]",
            out(reg) teb,
            options(nostack, preserves_flags, readonly)
        );
        // SAFETY: `TEB + 0x1780` is the `TlsExpansionSlots` pointer field; read
        // the (possibly null) expansion-array base.
        let expansion_slots = *(teb.add(0x1780) as *mut *mut *mut core::ffi::c_void);
        if !expansion_slots.is_null() {
            // SAFETY: the array is non-null (just checked) and covers every
            // allocated index >= 64, so `index - 64` is an in-bounds slot.
            *expansion_slots.add(index as usize - 64) = value;
        }
    }
}

#[cfg(not(nightly_tls_active))]
#[inline(always)]
pub(crate) fn get_profiler_state() -> *mut ThreadState {
    #[cfg(any(feature = "std_tls", miri))]
    {
        THREAD_STATE.with(|cell| cell.get())
    }
    #[cfg(all(not(feature = "std_tls"), not(miri)))]
    {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            let Some(key) = get_os_tls_key(&PROFILER_TLS_KEY) else {
                return THREAD_STATE.with(|cell| cell.get());
            };
            // SAFETY: `key` is the profiler's own `TlsAlloc`-allocated slot index.
            let ptr = unsafe { get_teb_tls_slot(key) } as *mut ThreadState;
            if !ptr.is_null() {
                ptr
            } else {
                THREAD_STATE.with(|cell| {
                    let p = cell.get();
                    // SAFETY: `key` is the profiler's allocated slot; we publish
                    // this thread's own `THREAD_STATE` cell pointer into it so
                    // future reads on this thread reuse the same state.
                    unsafe { set_teb_tls_slot(key, p as *mut core::ffi::c_void) };
                    p
                })
            }
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            let Some(key) = get_os_tls_key(&PROFILER_TLS_KEY) else {
                return THREAD_STATE.with(|cell| cell.get());
            };
            let ptr = get_os_tls_value(key) as *mut ThreadState;
            if !ptr.is_null() {
                ptr
            } else {
                THREAD_STATE.with(|cell| {
                    let p = cell.get();
                    set_os_tls_value(key, p as *mut core::ffi::c_void);
                    p
                })
            }
        }
    }
}

#[inline(always)]
pub(crate) fn should_skip_alloc_fast_path(
    size: usize,
    hook_absent: bool,
    leak_inactive: bool,
) -> bool {
    // SAFETY: `THREAD_STATE` is this thread's own `#[thread_local]` static, so
    // the reentrancy check and `bytes_until_sample` update cannot race another
    // thread; the `in_hook` guard prevents nested mutation within the thread.
    #[cfg(nightly_tls_active)]
    unsafe {
        should_skip_alloc_fast_path_state(&mut THREAD_STATE, size, hook_absent, leak_inactive)
    }
    // SAFETY: `get_profiler_state()` returns this thread's own thread-local
    // `ThreadState`; the `&mut` is exclusive (thread-local) and the `in_hook`
    // check below rejects re-entry before any nested `&mut` could form.
    #[cfg(not(nightly_tls_active))]
    unsafe {
        should_skip_alloc_fast_path_state(
            &mut *get_profiler_state(),
            size,
            hook_absent,
            leak_inactive,
        )
    }
}

#[inline(always)]
fn should_skip_alloc_fast_path_state(
    state: &mut ThreadState,
    size: usize,
    hook_absent: bool,
    leak_inactive: bool,
) -> bool {
    if state.in_hook {
        return true;
    }

    if hook_absent && leak_inactive {
        let debit = sample_debit(size);
        if state.bytes_until_sample > debit {
            state.bytes_until_sample -= debit;
            return true;
        }
    }

    false
}

#[inline(always)]
pub(crate) fn enter_hook() -> bool {
    // SAFETY: `THREAD_STATE` is a `#[thread_local]` static owned exclusively by
    // the current thread, so the read-modify-write of `in_hook` cannot race
    // another thread; it is the guard that establishes single-entry, so no
    // nested `&mut` to the state is live while this runs.
    #[cfg(nightly_tls_active)]
    unsafe {
        if THREAD_STATE.in_hook {
            true
        } else {
            THREAD_STATE.in_hook = true;
            false
        }
    }
    // SAFETY: `get_profiler_state()` returns this thread's own thread-local
    // `ThreadState`; the pointee is exclusive to the current thread, and this
    // call is the re-entrancy guard itself, so no other `&mut` to it is live.
    #[cfg(not(nightly_tls_active))]
    unsafe {
        let state = &mut *get_profiler_state();
        if state.in_hook {
            true
        } else {
            state.in_hook = true;
            false
        }
    }
}

#[inline(always)]
pub(crate) fn exit_hook() {
    // SAFETY: `THREAD_STATE` is this thread's own `#[thread_local]` static;
    // clearing `in_hook` is an exclusive thread-local write.
    #[cfg(nightly_tls_active)]
    unsafe {
        THREAD_STATE.in_hook = false;
    }
    // SAFETY: `get_profiler_state()` returns this thread's own thread-local
    // state; clearing `in_hook` through it is an exclusive thread-local write
    // paired with the `enter_hook` that set it.
    #[cfg(not(nightly_tls_active))]
    unsafe {
        (*get_profiler_state()).in_hook = false;
    }
}

#[cfg(nightly_tls_active)]
#[inline(always)]
pub(crate) fn get_bytes_until_sample() -> isize {
    // SAFETY: `THREAD_STATE` is this thread's own `#[thread_local]` static; the
    // read of `bytes_until_sample` cannot race another thread.
    unsafe { THREAD_STATE.bytes_until_sample }
}

#[cfg(nightly_tls_active)]
#[inline(always)]
pub(crate) fn set_bytes_until_sample(val: isize) {
    // SAFETY: `THREAD_STATE` is this thread's own `#[thread_local]` static; the
    // write to `bytes_until_sample` is an exclusive thread-local store.
    unsafe {
        THREAD_STATE.bytes_until_sample = val;
    }
}
