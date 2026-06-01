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

#[cfg(feature = "nightly_tls")]
#[thread_local]
static mut BYTES_UNTIL_SAMPLE: isize = 0;

#[cfg(feature = "nightly_tls")]
#[thread_local]
static mut IN_HOOK: bool = false;

#[cfg(not(feature = "nightly_tls"))]
std::thread_local! {
    static BYTES_UNTIL_SAMPLE: core::cell::Cell<isize> = const { core::cell::Cell::new(0) };
    static IN_HOOK: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

#[inline(always)]
pub(crate) fn enter_hook() -> bool {
    #[cfg(feature = "nightly_tls")]
    unsafe {
        if IN_HOOK {
            true
        } else {
            IN_HOOK = true;
            false
        }
    }
    #[cfg(not(feature = "nightly_tls"))]
    IN_HOOK.with(|cell| {
        if cell.get() {
            true
        } else {
            cell.set(true);
            false
        }
    })
}

#[inline(always)]
pub(crate) fn exit_hook() {
    #[cfg(feature = "nightly_tls")]
    unsafe {
        IN_HOOK = false;
    }
    #[cfg(not(feature = "nightly_tls"))]
    IN_HOOK.with(|cell| cell.set(false));
}

#[cfg(feature = "nightly_tls")]
#[inline(always)]
pub(crate) fn get_bytes_until_sample() -> isize {
    unsafe { BYTES_UNTIL_SAMPLE }
}

#[cfg(feature = "nightly_tls")]
#[inline(always)]
pub(crate) fn set_bytes_until_sample(val: isize) {
    unsafe {
        BYTES_UNTIL_SAMPLE = val;
    }
}

#[cfg(not(feature = "nightly_tls"))]
#[inline(always)]
pub(crate) fn with_bytes_until_sample<R>(f: impl FnOnce(&core::cell::Cell<isize>) -> R) -> R {
    BYTES_UNTIL_SAMPLE.with(f)
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
        if IN_HOOK {
            return;
        }
        let hook_ptr = ALLOC_HOOK.load(Ordering::Relaxed);
        let leak_active = LEAK_DETECTOR_ACTIVE.load(Ordering::Relaxed);
        if hook_ptr.is_null() && !leak_active {
            let val = BYTES_UNTIL_SAMPLE;
            if val > size as isize {
                BYTES_UNTIL_SAMPLE = val - size as isize;
                return;
            }
        }
    }

    #[cfg(not(feature = "nightly_tls"))]
    {
        let is_fast = IN_HOOK.with(|in_hook_cell| {
            if in_hook_cell.get() {
                return Some(());
            }
            let hook_ptr = ALLOC_HOOK.load(Ordering::Relaxed);
            let leak_active = LEAK_DETECTOR_ACTIVE.load(Ordering::Relaxed);
            if hook_ptr.is_null() && !leak_active {
                let val = BYTES_UNTIL_SAMPLE.with(|val_cell| val_cell.get());
                if val > size as isize {
                    BYTES_UNTIL_SAMPLE.with(|val_cell| val_cell.set(val - size as isize));
                    return Some(());
                }
            }
            None
        });
        if is_fast.is_some() {
            return;
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
