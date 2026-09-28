use core::ffi::{CStr, c_char, c_void};

/// Registers a custom user allocation tracing hook.
///
/// # Safety
///
/// `hook` must be a valid function pointer adhering to the C calling convention,
/// or `None` to unregister. The hook is invoked on every allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mnemosyne_register_alloc_hook(
    hook: Option<unsafe extern "C" fn(*mut c_void, usize)>,
) {
    mnemosyne_prof::register_alloc_hook(hook);
}

/// Registers a custom user deallocation tracing hook.
///
/// # Safety
///
/// `hook` must be a valid function pointer adhering to the C calling convention,
/// or `None` to unregister. The hook is invoked on every deallocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mnemosyne_register_free_hook(
    hook: Option<unsafe extern "C" fn(*mut c_void, usize)>,
) {
    mnemosyne_prof::register_free_hook(hook);
}

/// Enables the built-in Poisson heap sampler.
#[unsafe(no_mangle)]
pub extern "C" fn mnemosyne_enable_profiling(sample_interval: usize) {
    mnemosyne_prof::enable_profiling(sample_interval);
}

/// Disables the built-in Poisson heap sampler.
#[unsafe(no_mangle)]
pub extern "C" fn mnemosyne_disable_profiling() {
    mnemosyne_prof::disable_profiling();
}

/// Returns whether the built-in heap sampler is currently active.
#[unsafe(no_mangle)]
pub extern "C" fn mnemosyne_is_profiling_enabled() -> i32 {
    i32::from(mnemosyne_prof::is_profiling_enabled())
}

/// Dumps a folded stack profile of active memory allocations to a file.
///
/// Returns 0 on success, or -1 on error.
///
/// # Safety
///
/// `path` must be a valid null-terminated UTF-8 C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mnemosyne_dump_profile(path: *const c_char) -> i32 {
    if path.is_null() {
        return -1;
    }
    // SAFETY: path must be a valid null-terminated C string.
    let c_str = unsafe { CStr::from_ptr(path) };
    let Ok(str_slice) = c_str.to_str() else {
        return -1;
    };
    match mnemosyne_prof::dump_profile(str_slice) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Resets the profiler state, trace hooks, and sampled data. Intended for testing.
#[unsafe(no_mangle)]
pub extern "C" fn mnemosyne_reset_profiler_for_testing() {
    mnemosyne_prof::reset_profiler_for_testing();
}

/// Enables the built-in memory leak detector, tracking every allocation with its backtrace.
#[unsafe(no_mangle)]
pub extern "C" fn mnemosyne_enable_leak_detector() {
    mnemosyne_prof::enable_leak_detector();
}

/// Disables the built-in memory leak detector.
#[unsafe(no_mangle)]
pub extern "C" fn mnemosyne_disable_leak_detector() {
    mnemosyne_prof::disable_leak_detector();
}

/// Returns whether the memory leak detector is currently active (1 if active, 0 if inactive).
#[unsafe(no_mangle)]
pub extern "C" fn mnemosyne_is_leak_detector_enabled() -> i32 {
    i32::from(mnemosyne_prof::is_leak_detector_enabled())
}

/// Dumps a report of all active memory allocations (leaks) to the specified file path.
///
/// Returns the number of leaks written on success, or -1 on error. Counts
/// above `i32::MAX` saturate to `i32::MAX` (a wrapping cast could collide
/// with the -1 error sentinel).
///
/// # Safety
///
/// `path` must be a valid, null-terminated UTF-8 C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mnemosyne_dump_leaks(path: *const c_char) -> i32 {
    if path.is_null() {
        return -1;
    }
    // SAFETY: path must be a valid null-terminated C string.
    let c_str = unsafe { CStr::from_ptr(path) };
    let Ok(str_slice) = c_str.to_str() else {
        return -1;
    };
    match mnemosyne_prof::dump_leaks(str_slice) {
        Ok(count) => i32::try_from(count).unwrap_or(i32::MAX),
        Err(_) => -1,
    }
}
