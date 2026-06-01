static OPTIONS_INIT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[cfg(windows)]
fn get_env_var_stack(name: &str, buf: &mut [u8]) -> Option<usize> {
    extern "system" {
        fn GetEnvironmentVariableA(lpName: *const u8, lpBuffer: *mut u8, nSize: u32) -> u32;
    }

    let mut name_buf = [0u8; 64];
    if name.len() >= name_buf.len() {
        return None;
    }
    name_buf[..name.len()].copy_from_slice(name.as_bytes());
    name_buf[name.len()] = 0;

    let res =
        unsafe { GetEnvironmentVariableA(name_buf.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };

    if res == 0 || res >= buf.len() as u32 {
        None
    } else {
        Some(res as usize)
    }
}

#[cfg(not(windows))]
fn get_env_var_stack(name: &str, buf: &mut [u8]) -> Option<usize> {
    extern "C" {
        fn getenv(name: *const u8) -> *mut u8;
    }

    let mut name_buf = [0u8; 64];
    if name.len() >= name_buf.len() {
        return None;
    }
    name_buf[..name.len()].copy_from_slice(name.as_bytes());
    name_buf[name.len()] = 0;

    let ptr = unsafe { getenv(name_buf.as_ptr()) };
    if ptr.is_null() {
        return None;
    }

    let mut len = 0;
    unsafe {
        while *ptr.add(len) != 0 && len < buf.len() {
            buf[len] = *ptr.add(len);
            len += 1;
        }
    }
    if len == buf.len() {
        None
    } else {
        Some(len)
    }
}

fn parse_env_usize(name: &str) -> Option<usize> {
    let mut buf = [0u8; 32];
    let len = get_env_var_stack(name, &mut buf)?;
    let s = core::str::from_utf8(&buf[..len]).ok()?;
    s.trim().parse::<usize>().ok()
}

fn parse_env_bool(name: &str) -> Option<bool> {
    let mut buf = [0u8; 32];
    let len = get_env_var_stack(name, &mut buf)?;
    let s = core::str::from_utf8(&buf[..len]).ok()?.trim();
    if s.eq_ignore_ascii_case("true") || s == "1" {
        Some(true)
    } else if s.eq_ignore_ascii_case("false") || s == "0" {
        Some(false)
    } else {
        None
    }
}

#[inline(always)]
pub fn ensure_options_initialized() {
    if !OPTIONS_INIT.load(core::sync::atomic::Ordering::Acquire) {
        init_options_from_env();
    }
}

#[cold]
#[inline(never)]
fn init_options_from_env() {
    if OPTIONS_INIT.swap(true, core::sync::atomic::Ordering::Acquire) {
        return;
    }

    if let Some(parsed) = parse_env_usize("MNEMOSYNE_MAX_RETAINED_SEGMENTS") {
        let clamped = core::cmp::min(parsed, 1024);
        mnemosyne_core::options::MAX_RETAINED_SEGMENTS
            .store(clamped, core::sync::atomic::Ordering::Release);
    }

    if let Some(parsed) = parse_env_bool("MNEMOSYNE_ENABLE_HUGEPAGE_HINT") {
        mnemosyne_core::options::ENABLE_HUGEPAGE_HINT
            .store(parsed, core::sync::atomic::Ordering::Release);
    }

    if let Some(parsed) = parse_env_usize("MNEMOSYNE_PURGE_CADENCE_MS") {
        mnemosyne_core::options::PURGE_CADENCE_MS
            .store(parsed, core::sync::atomic::Ordering::Release);
        if parsed > 0 {
            mnemosyne_decay::init_decay_engine();
        }
    }

    if let Some(parsed) = parse_env_bool("MNEMOSYNE_PROF") {
        if parsed {
            let interval = parse_env_usize("MNEMOSYNE_PROF_SAMPLE_INTERVAL").unwrap_or(512 * 1024);
            mnemosyne_prof::enable_profiling(interval);
        }
    }

    if let Some(parsed) = parse_env_bool("MNEMOSYNE_LEAK_DETECTOR") {
        if parsed {
            mnemosyne_prof::enable_leak_detector();
        }
    }
}

/// Reset options state and atomic option values to their defaults. Intended for testing.
#[doc(hidden)]
pub fn reset_options_for_testing() {
    OPTIONS_INIT.store(false, core::sync::atomic::Ordering::Release);
    mnemosyne_core::options::MAX_RETAINED_SEGMENTS
        .store(1024, core::sync::atomic::Ordering::Release);
    mnemosyne_core::options::ENABLE_HUGEPAGE_HINT
        .store(true, core::sync::atomic::Ordering::Release);
    mnemosyne_core::options::PURGE_CADENCE_MS.store(0, core::sync::atomic::Ordering::Release);
    mnemosyne_prof::reset_profiler_for_testing();
}

/// Marks options as initialized, preventing subsequent environment parsing from overwriting them.
#[doc(hidden)]
pub fn mark_options_initialized() {
    OPTIONS_INIT.store(true, core::sync::atomic::Ordering::Release);
}
