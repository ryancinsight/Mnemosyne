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
mod os_key;
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
            let Some(key) = os_key::get_os_tls_key(&os_key::PROFILER_TLS_KEY) else {
                return THREAD_STATE.with(|cell| cell.get());
            };
            // SAFETY: `key` is the profiler's own `TlsAlloc`-allocated slot index.
            let ptr = unsafe { os_key::get_teb_tls_slot(key) } as *mut ThreadState;
            if !ptr.is_null() {
                ptr
            } else {
                THREAD_STATE.with(|cell| {
                    let p = cell.get();
                    // SAFETY: `key` is the profiler's allocated slot; we publish
                    // this thread's own `THREAD_STATE` cell pointer into it so
                    // future reads on this thread reuse the same state.
                    unsafe { os_key::set_teb_tls_slot(key, p as *mut core::ffi::c_void) };
                    p
                })
            }
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            let Some(key) = os_key::get_os_tls_key(&os_key::PROFILER_TLS_KEY) else {
                return THREAD_STATE.with(|cell| cell.get());
            };
            let ptr = os_key::get_os_tls_value(key) as *mut ThreadState;
            if !ptr.is_null() {
                ptr
            } else {
                THREAD_STATE.with(|cell| {
                    let p = cell.get();
                    os_key::set_os_tls_value(key, p as *mut core::ffi::c_void);
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
    // `&raw mut` sidesteps a direct `static mut` reference (`static_mut_refs`
    // is deny-by-default in edition 2024); the exclusive reborrow is sound
    // because the static is thread-local and no other reference to it is live
    // across this call.
    #[cfg(nightly_tls_active)]
    unsafe {
        should_skip_alloc_fast_path_state(
            &mut *(&raw mut THREAD_STATE),
            size,
            hook_absent,
            leak_inactive,
        )
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
