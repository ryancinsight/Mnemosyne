#![cfg_attr(feature = "nightly_tls", feature(thread_local))]
#![allow(clippy::missing_const_for_thread_local)]

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

mod sampler;
#[cfg(test)]
mod tests;

pub use sampler::{dump_leaks, dump_profile, Sample};

static ALLOC_HOOK: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static FREE_HOOK: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

static PROFILING_ACTIVE: AtomicBool = AtomicBool::new(false);
static LEAK_DETECTOR_ACTIVE: AtomicBool = AtomicBool::new(false);
static PROFILING_OR_HOOKS_ACTIVE: AtomicBool = AtomicBool::new(false);
static SAMPLE_INTERVAL: AtomicUsize = AtomicUsize::new(512 * 1024); // Default 512 KB
static ACTIVE_SAMPLES_COUNT: AtomicUsize = AtomicUsize::new(0);

fn update_active_flag() {
    let active = PROFILING_ACTIVE.load(Ordering::Acquire)
        || LEAK_DETECTOR_ACTIVE.load(Ordering::Acquire)
        || !ALLOC_HOOK.load(Ordering::Acquire).is_null()
        || !FREE_HOOK.load(Ordering::Acquire).is_null();
    PROFILING_OR_HOOKS_ACTIVE.store(active, Ordering::Release);
}

/// Registers a custom user allocation tracing hook.
pub fn register_alloc_hook(hook: Option<unsafe extern "C" fn(*mut core::ffi::c_void, usize)>) {
    let ptr = match hook {
        Some(f) => f as *mut c_void,
        None => core::ptr::null_mut(),
    };
    ALLOC_HOOK.store(ptr, Ordering::Release);
    update_active_flag();
}

/// Registers a custom user deallocation tracing hook.
pub fn register_free_hook(hook: Option<unsafe extern "C" fn(*mut core::ffi::c_void, usize)>) {
    let ptr = match hook {
        Some(f) => f as *mut c_void,
        None => core::ptr::null_mut(),
    };
    FREE_HOOK.store(ptr, Ordering::Release);
    update_active_flag();
}

/// Enables the built-in Poisson heap sampler.
pub fn enable_profiling(sample_interval: usize) {
    SAMPLE_INTERVAL.store(sample_interval, Ordering::Release);
    PROFILING_ACTIVE.store(true, Ordering::Release);
    update_active_flag();
}

/// Disables the built-in Poisson heap sampler.
pub fn disable_profiling() {
    PROFILING_ACTIVE.store(false, Ordering::Release);
    update_active_flag();
}

/// Returns whether the built-in heap sampler is currently active.
pub fn is_profiling_enabled() -> bool {
    PROFILING_ACTIVE.load(Ordering::Acquire)
}

/// Resets the profiler state, trace hooks, and sampled data. Intended for testing.
pub fn reset_profiler_for_testing() {
    PROFILING_ACTIVE.store(false, Ordering::Release);
    LEAK_DETECTOR_ACTIVE.store(false, Ordering::Release);
    ALLOC_HOOK.store(core::ptr::null_mut(), Ordering::Release);
    FREE_HOOK.store(core::ptr::null_mut(), Ordering::Release);
    SAMPLE_INTERVAL.store(512 * 1024, Ordering::Release);
    ACTIVE_SAMPLES_COUNT.store(0, Ordering::Release);
    sampler::reset_sampler_state();
    update_active_flag();
}

/// Enables the built-in memory leak detector, tracking every allocation with its backtrace.
pub fn enable_leak_detector() {
    LEAK_DETECTOR_ACTIVE.store(true, Ordering::Release);
    update_active_flag();
}

/// Disables the built-in memory leak detector.
pub fn disable_leak_detector() {
    LEAK_DETECTOR_ACTIVE.store(false, Ordering::Release);
    update_active_flag();
}

/// Returns whether the memory leak detector is currently active.
pub fn is_leak_detector_enabled() -> bool {
    LEAK_DETECTOR_ACTIVE.load(Ordering::Acquire)
}

#[derive(Clone, Copy)]
pub(crate) struct ThreadState {
    pub(crate) bytes_until_sample: isize,
    pub(crate) in_hook: bool,
}

#[cfg(feature = "nightly_tls")]
#[thread_local]
static mut THREAD_STATE: ThreadState = ThreadState {
    bytes_until_sample: 0,
    in_hook: false,
};

#[cfg(not(feature = "nightly_tls"))]
std::thread_local! {
    static THREAD_STATE: core::cell::UnsafeCell<ThreadState> = const {
        core::cell::UnsafeCell::new(ThreadState {
            bytes_until_sample: 0,
            in_hook: false,
        })
    };
}

#[cfg(not(feature = "nightly_tls"))]
static PROFILER_TLS_KEY: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(u32::MAX);

#[cfg(not(feature = "nightly_tls"))]
#[inline(always)]
fn get_os_tls_key(atomic_key: &core::sync::atomic::AtomicU32) -> Option<u32> {
    let mut key = atomic_key.load(Ordering::Acquire);
    if key == u32::MAX {
        key = init_os_tls_key(atomic_key)?;
    }
    Some(key)
}

#[cfg(not(feature = "nightly_tls"))]
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
            match atomic_key.compare_exchange(u32::MAX, key, Ordering::AcqRel, Ordering::Acquire) {
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
            match atomic_key.compare_exchange(u32::MAX, key, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => Some(key),
                Err(existing) => {
                    pthread_key_delete(key);
                    (existing != u32::MAX).then_some(existing)
                }
            }
        }
    }
}

#[cfg(not(feature = "nightly_tls"))]
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

#[cfg(not(feature = "nightly_tls"))]
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

#[cfg(all(not(feature = "nightly_tls"), all(windows, target_arch = "x86_64")))]
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

#[cfg(all(not(feature = "nightly_tls"), all(windows, target_arch = "x86_64")))]
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

#[cfg(not(feature = "nightly_tls"))]
#[inline(always)]
pub(crate) fn get_profiler_state() -> *mut ThreadState {
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

#[inline(always)]
pub(crate) fn enter_hook() -> bool {
    #[cfg(feature = "nightly_tls")]
    unsafe {
        if THREAD_STATE.in_hook {
            true
        } else {
            THREAD_STATE.in_hook = true;
            false
        }
    }
    #[cfg(not(feature = "nightly_tls"))]
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
    #[cfg(feature = "nightly_tls")]
    unsafe {
        THREAD_STATE.in_hook = false;
    }
    #[cfg(not(feature = "nightly_tls"))]
    unsafe {
        (*get_profiler_state()).in_hook = false;
    }
}

#[cfg(feature = "nightly_tls")]
#[inline(always)]
pub(crate) fn get_bytes_until_sample() -> isize {
    unsafe { THREAD_STATE.bytes_until_sample }
}

#[cfg(feature = "nightly_tls")]
#[inline(always)]
pub(crate) fn set_bytes_until_sample(val: isize) {
    unsafe {
        THREAD_STATE.bytes_until_sample = val;
    }
}

/// Returns whether any tracing hooks or profiling sessions are currently active.
#[inline(always)]
pub fn is_active() -> bool {
    PROFILING_OR_HOOKS_ACTIVE.load(Ordering::Relaxed)
}

/// Entry point invoked on every successful memory allocation.
///
/// Calls any registered custom user hook and registers a sample if the
/// Poisson heap sampler is active.
#[inline(always)]
pub fn on_alloc(ptr: *mut u8, size: usize) {
    if !PROFILING_OR_HOOKS_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    #[cfg(feature = "nightly_tls")]
    unsafe {
        if THREAD_STATE.in_hook {
            return;
        }
        let hook_ptr = ALLOC_HOOK.load(Ordering::Relaxed);
        let leak_active = LEAK_DETECTOR_ACTIVE.load(Ordering::Relaxed);
        if hook_ptr.is_null() && !leak_active {
            let val = THREAD_STATE.bytes_until_sample;
            if val > size as isize {
                THREAD_STATE.bytes_until_sample = val - size as isize;
                return;
            }
        }
    }

    #[cfg(not(feature = "nightly_tls"))]
    unsafe {
        let state = &mut *get_profiler_state();
        if state.in_hook {
            return;
        }
        let hook_ptr = ALLOC_HOOK.load(Ordering::Relaxed);
        let leak_active = LEAK_DETECTOR_ACTIVE.load(Ordering::Relaxed);
        if hook_ptr.is_null() && !leak_active {
            let val = state.bytes_until_sample;
            if val > size as isize {
                state.bytes_until_sample = val - size as isize;
                return;
            }
        }
    }

    on_alloc_cold(ptr, size);
}

#[inline(never)]
fn on_alloc_cold(ptr: *mut u8, size: usize) {
    if ptr.is_null() {
        return;
    }

    let hook_ptr = ALLOC_HOOK.load(Ordering::Relaxed);
    let active = PROFILING_ACTIVE.load(Ordering::Relaxed);
    let leak_active = LEAK_DETECTOR_ACTIVE.load(Ordering::Relaxed);
    if hook_ptr.is_null() && !active && !leak_active {
        return;
    }

    let in_hook = enter_hook();
    if in_hook {
        return;
    }

    if !hook_ptr.is_null() {
        let hook: unsafe extern "C" fn(*mut core::ffi::c_void, usize) =
            unsafe { core::mem::transmute(hook_ptr) };
        unsafe { hook(ptr as *mut core::ffi::c_void, size) };
    }

    if active || leak_active {
        sampler::sample_alloc_inner(ptr, size, leak_active);
    }

    exit_hook();
}

/// Entry point invoked on every successful memory deallocation.
///
/// Calls any registered custom user hook and removes the sampled allocation
/// if active.
#[inline(always)]
pub fn on_free(ptr: *mut u8, size: usize) {
    if !PROFILING_OR_HOOKS_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    let hook_ptr = FREE_HOOK.load(Ordering::Relaxed);
    if hook_ptr.is_null() && ACTIVE_SAMPLES_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }

    on_free_cold(ptr, size);
}

#[inline(never)]
fn on_free_cold(ptr: *mut u8, size: usize) {
    if ptr.is_null() {
        return;
    }

    let hook_ptr = FREE_HOOK.load(Ordering::Relaxed);
    let active = PROFILING_ACTIVE.load(Ordering::Relaxed);
    let leak_active = LEAK_DETECTOR_ACTIVE.load(Ordering::Relaxed);
    if hook_ptr.is_null() && !active && !leak_active {
        return;
    }

    let in_hook = enter_hook();
    if in_hook {
        return;
    }

    if !hook_ptr.is_null() {
        let hook: unsafe extern "C" fn(*mut core::ffi::c_void, usize) =
            unsafe { core::mem::transmute(hook_ptr) };
        unsafe { hook(ptr as *mut core::ffi::c_void, size) };
    }

    if (active || leak_active) && ACTIVE_SAMPLES_COUNT.load(Ordering::Relaxed) > 0 {
        sampler::sample_free_inner(ptr);
    }

    exit_hook();
}
