#[cfg(all(not(nightly_tls_active), not(feature = "std_tls"), not(miri)))]
use core::sync::atomic::Ordering;

#[derive(Clone, Copy)]
pub(crate) struct ThreadState {
    pub(crate) bytes_until_sample: isize,
    pub(crate) in_hook: bool,
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
#[inline(always)]
unsafe fn get_teb_tls_slot(index: u32) -> *mut core::ffi::c_void {
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

#[cfg(all(
    not(nightly_tls_active),
    not(feature = "std_tls"),
    all(windows, target_arch = "x86_64"),
    not(miri)
))]
#[inline(always)]
unsafe fn set_teb_tls_slot(index: u32, value: *mut core::ffi::c_void) {
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
            let ptr = unsafe { get_teb_tls_slot(key) } as *mut ThreadState;
            if !ptr.is_null() {
                ptr
            } else {
                THREAD_STATE.with(|cell| {
                    let p = cell.get();
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
pub(crate) fn enter_hook() -> bool {
    #[cfg(nightly_tls_active)]
    unsafe {
        if THREAD_STATE.in_hook {
            true
        } else {
            THREAD_STATE.in_hook = true;
            false
        }
    }
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
    #[cfg(nightly_tls_active)]
    unsafe {
        THREAD_STATE.in_hook = false;
    }
    #[cfg(not(nightly_tls_active))]
    unsafe {
        (*get_profiler_state()).in_hook = false;
    }
}

#[cfg(nightly_tls_active)]
#[inline(always)]
pub(crate) fn get_bytes_until_sample() -> isize {
    unsafe { THREAD_STATE.bytes_until_sample }
}

#[cfg(nightly_tls_active)]
#[inline(always)]
pub(crate) fn set_bytes_until_sample(val: isize) {
    unsafe {
        THREAD_STATE.bytes_until_sample = val;
    }
}
