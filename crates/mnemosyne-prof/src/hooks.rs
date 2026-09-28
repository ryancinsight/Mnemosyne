//! Allocation/deallocation hot path: the per-event entry points the allocator
//! crates call on every alloc and free.
//!
//! Each entry point does one relaxed flag load and returns immediately on the
//! common inactive path; the cold work lives in the `#[inline(never)]`
//! `*_cold` helpers so the fast path stays small enough to inline.

use core::sync::atomic::Ordering;

use crate::control::{
    ALLOC_HOOK, FREE_HOOK, LEAK_DETECTOR_ACTIVE, PROFILING_ACTIVE, PROFILING_OR_HOOKS_ACTIVE,
};
use crate::tls::{enter_hook, exit_hook, should_skip_alloc_fast_path};

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
        crate::sampler::sample_alloc_inner(ptr, size, leak_active);
    }

    exit_hook();
}

/// Entry point invoked on every successful memory deallocation.
///
/// Calls any registered custom user hook and removes the sampled allocation
/// if one is resident. Sample removal runs whenever resident samples exist —
/// even after profiling/leak detection has been disabled — so stale samples
/// drain on free instead of being reported as leaks by a later
/// [`dump_leaks`][crate::dump_leaks].
#[inline(always)]
pub fn on_free(ptr: *mut u8, size: usize) {
    // The cold path has work exactly when a free hook is registered or a
    // resident sample may need eviction; the profiling/leak flags are
    // irrelevant to frees (see `on_free_cold`).
    if FREE_HOOK.load(Ordering::Relaxed).is_null()
        && !crate::sampler::has_active_sample_for(ptr as usize)
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
    // block and (b) leave the pointer's occupancy flag set forever, taxing
    // every subsequent free in that shard with this cold call. Gate on
    // resident samples instead.
    let samples_resident = crate::sampler::has_active_sample_for(ptr as usize);
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
        crate::sampler::sample_free_inner(ptr);
    }

    exit_hook();
}
