#![cfg_attr(nightly_tls_active, feature(thread_local))]
#![allow(clippy::missing_const_for_thread_local)]

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

mod sampler;
#[cfg(test)]
mod tests;
mod tls;

pub use sampler::{dump_leaks, dump_profile, Sample, StackId};

pub(crate) use tls::{
    enter_hook, exit_hook, get_profiler_state, sample_debit, should_skip_alloc_fast_path,
};
#[cfg(nightly_tls_active)]
pub(crate) use tls::{get_bytes_until_sample, set_bytes_until_sample};

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

    let hook_ptr = ALLOC_HOOK.load(Ordering::Relaxed);
    let leak_active = LEAK_DETECTOR_ACTIVE.load(Ordering::Relaxed);
    if should_skip_alloc_fast_path(size, hook_ptr.is_null(), leak_active) {
        return;
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
        // SAFETY: `hook_ptr` is non-null (just checked) and was published by
        // `register_alloc_hook` as `f as *mut c_void` from a real
        // `unsafe extern "C" fn(*mut c_void, usize)` under `Release`/`Acquire`
        // ordering, so transmuting it back to that exact signature reconstructs
        // a valid function pointer.
        let hook: unsafe extern "C" fn(*mut core::ffi::c_void, usize) =
            unsafe { core::mem::transmute(hook_ptr) };
        // SAFETY: `ptr`/`size` are the just-completed allocation's address and
        // size; the registered hook upholds its own `extern "C"` contract.
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
        // SAFETY: `hook_ptr` is non-null and was published by
        // `register_free_hook` as `f as *mut c_void` from a real
        // `unsafe extern "C" fn(*mut c_void, usize)`, so transmuting it back to
        // that exact signature reconstructs a valid function pointer.
        let hook: unsafe extern "C" fn(*mut core::ffi::c_void, usize) =
            unsafe { core::mem::transmute(hook_ptr) };
        // SAFETY: `ptr`/`size` describe the allocation being freed; the
        // registered hook upholds its own `extern "C"` contract.
        unsafe { hook(ptr as *mut core::ffi::c_void, size) };
    }

    if (active || leak_active) && ACTIVE_SAMPLES_COUNT.load(Ordering::Relaxed) > 0 {
        sampler::sample_free_inner(ptr);
    }

    exit_hook();
}
