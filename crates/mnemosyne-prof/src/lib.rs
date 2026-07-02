//! Heap profiling runtime for the Mnemosyne allocator: user alloc/free trace
//! hooks, a Poisson heap sampler, and an every-allocation leak detector, all
//! reached through the `on_alloc`/`on_free` entry points the allocator crates
//! call on every allocation and deallocation.

#![cfg_attr(nightly_tls_active, feature(thread_local))]
#![allow(clippy::missing_const_for_thread_local)]
#![deny(missing_docs)]

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

/// Serializes control-plane updates (hook registration, sampler and
/// leak-detector enable/disable) with the recompute of
/// `PROFILING_OR_HOOKS_ACTIVE`.
///
/// Without it the recompute has a lost-update race: registrar A stores its
/// flag and computes the aggregate; registrar B stores its flag, computes,
/// and stores its aggregate; then A stores an aggregate that was computed
/// *before* B's flag store — stranding `PROFILING_OR_HOOKS_ACTIVE` stale
/// (hooks that silently never fire, or a permanent fast-path tax). Holding
/// the lock across (flag store + recompute + aggregate store) orders the
/// critical sections, so whichever registrar recomputes last observes every
/// earlier flag store. Control-plane only — never taken on alloc/free paths
/// — so a mutex is appropriate.
static UPDATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Runs `store` (the registrar's own flag store) and the aggregate recompute
/// as one critical section under [`UPDATE_LOCK`].
fn store_flags_then_update_active(store: impl FnOnce()) {
    let _guard = UPDATE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store();
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
    store_flags_then_update_active(|| ALLOC_HOOK.store(ptr, Ordering::Release));
}

/// Registers a custom user deallocation tracing hook.
pub fn register_free_hook(hook: Option<unsafe extern "C" fn(*mut core::ffi::c_void, usize)>) {
    let ptr = match hook {
        Some(f) => f as *mut c_void,
        None => core::ptr::null_mut(),
    };
    store_flags_then_update_active(|| FREE_HOOK.store(ptr, Ordering::Release));
}

/// Enables the built-in Poisson heap sampler.
pub fn enable_profiling(sample_interval: usize) {
    store_flags_then_update_active(|| {
        SAMPLE_INTERVAL.store(sample_interval, Ordering::Release);
        PROFILING_ACTIVE.store(true, Ordering::Release);
    });
}

/// Disables the built-in Poisson heap sampler.
pub fn disable_profiling() {
    store_flags_then_update_active(|| PROFILING_ACTIVE.store(false, Ordering::Release));
}

/// Returns whether the built-in heap sampler is currently active.
pub fn is_profiling_enabled() -> bool {
    PROFILING_ACTIVE.load(Ordering::Acquire)
}

/// Resets the profiler state, trace hooks, and sampled data. Intended for testing.
pub fn reset_profiler_for_testing() {
    store_flags_then_update_active(|| {
        PROFILING_ACTIVE.store(false, Ordering::Release);
        LEAK_DETECTOR_ACTIVE.store(false, Ordering::Release);
        ALLOC_HOOK.store(core::ptr::null_mut(), Ordering::Release);
        FREE_HOOK.store(core::ptr::null_mut(), Ordering::Release);
        SAMPLE_INTERVAL.store(512 * 1024, Ordering::Release);
        ACTIVE_SAMPLES_COUNT.store(0, Ordering::Release);
    });
    // Sampler-internal locks are taken outside `UPDATE_LOCK` to keep the
    // control-plane lock leaf-level (no nested acquisition order to maintain).
    sampler::reset_sampler_state();
}

/// Enables the built-in memory leak detector, tracking every allocation with its backtrace.
pub fn enable_leak_detector() {
    store_flags_then_update_active(|| LEAK_DETECTOR_ACTIVE.store(true, Ordering::Release));
}

/// Disables the built-in memory leak detector.
pub fn disable_leak_detector() {
    store_flags_then_update_active(|| LEAK_DETECTOR_ACTIVE.store(false, Ordering::Release));
}

/// Returns whether the memory leak detector is currently active.
pub fn is_leak_detector_enabled() -> bool {
    LEAK_DETECTOR_ACTIVE.load(Ordering::Acquire)
}

/// Returns whether any tracing hook, the heap sampler, or the leak detector
/// is currently active (the aggregate flag the allocator fast path checks).
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
    // The budgeted fast skip is only sound when the allocation needs no leak
    // tracking: pass the INACTIVE sense of the flag (a prior inversion here
    // let a stale sampling budget hide allocations from the leak detector).
    let leak_inactive = !LEAK_DETECTOR_ACTIVE.load(Ordering::Relaxed);
    if should_skip_alloc_fast_path(size, hook_ptr.is_null(), leak_inactive) {
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
/// if one is resident. Sample removal runs whenever resident samples exist —
/// even after profiling/leak detection has been disabled — so stale samples
/// drain on free instead of being reported as leaks by a later
/// [`dump_leaks`].
#[inline(always)]
pub fn on_free(ptr: *mut u8, size: usize) {
    // The cold path has work exactly when a free hook is registered or a
    // resident sample may need eviction; the profiling/leak flags are
    // irrelevant to frees (see `on_free_cold`).
    if FREE_HOOK.load(Ordering::Relaxed).is_null()
        && ACTIVE_SAMPLES_COUNT.load(Ordering::Relaxed) == 0
    {
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
    // Sample removal is state hygiene, not sampling: a block recorded while
    // the sampler or leak detector was active must still be evicted when it
    // is freed after those modes were disabled. Gating removal on the active
    // flags would (a) make a later `dump_leaks` falsely report the freed
    // block and (b) leave `ACTIVE_SAMPLES_COUNT` above zero forever, taxing
    // every subsequent free with this cold call. Gate on resident samples
    // instead.
    let samples_resident = ACTIVE_SAMPLES_COUNT.load(Ordering::Relaxed) > 0;
    if hook_ptr.is_null() && !samples_resident {
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

    if samples_resident {
        sampler::sample_free_inner(ptr);
    }

    exit_hook();
}
