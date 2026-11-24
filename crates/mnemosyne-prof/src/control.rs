//! Profiler control plane: mode flags, trace-hook slots, and the public
//! enable/disable/register entry points that keep the aggregate fast-path
//! flag coherent.

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};

use crate::{cast, sampler};

pub(crate) static ALLOC_HOOK: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
pub(crate) static FREE_HOOK: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

pub(crate) static PROFILING_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) static LEAK_DETECTOR_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) static PROFILING_OR_HOOKS_ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) static SAMPLE_INTERVAL: AtomicUsize = AtomicUsize::new(512 * 1024); // Default 512 KB

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
        Some(f) => cast::hook_slot_pointer(f),
        None => core::ptr::null_mut(),
    };
    store_flags_then_update_active(|| ALLOC_HOOK.store(ptr, Ordering::Release));
}

/// Registers a custom user deallocation tracing hook.
pub fn register_free_hook(hook: Option<unsafe extern "C" fn(*mut core::ffi::c_void, usize)>) {
    let ptr = match hook {
        Some(f) => cast::hook_slot_pointer(f),
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
