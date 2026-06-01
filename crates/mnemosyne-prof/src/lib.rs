#![cfg_attr(feature = "nightly_tls", feature(thread_local))]
#![allow(clippy::missing_const_for_thread_local)]

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering};
use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;

static ALLOC_HOOK: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static FREE_HOOK: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

static PROFILING_ACTIVE: AtomicBool = AtomicBool::new(false);
static LEAK_DETECTOR_ACTIVE: AtomicBool = AtomicBool::new(false);
static PROFILING_OR_HOOKS_ACTIVE: AtomicBool = AtomicBool::new(false);
static SAMPLE_INTERVAL: AtomicUsize = AtomicUsize::new(512 * 1024); // Default 512 KB

/// Representation of a sampled memory allocation.
pub struct Sample {
    /// Allocated size of the block in bytes.
    pub size: usize,
    /// Stack trace represented as instruction pointers.
    pub stack: Vec<usize>,
}

const SHARDS: usize = 64;
static ACTIVE_SAMPLES: [Mutex<Option<HashMap<usize, Sample>>>; SHARDS] =
    [const { Mutex::new(None) }; SHARDS];

fn get_map(shard: usize) -> std::sync::MutexGuard<'static, Option<HashMap<usize, Sample>>> {
    let mut lock = ACTIVE_SAMPLES[shard].lock().unwrap();
    if lock.is_none() {
        *lock = Some(HashMap::new());
    }
    lock
}

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
    for shard in &ACTIVE_SAMPLES {
        let mut lock = shard.lock().unwrap();
        *lock = None;
    }
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

    #[cfg(feature = "nightly_tls")]
    let in_hook = unsafe {
        if IN_HOOK {
            true
        } else {
            IN_HOOK = true;
            false
        }
    };
    #[cfg(not(feature = "nightly_tls"))]
    let in_hook = IN_HOOK.with(|cell| {
        if cell.get() {
            true
        } else {
            cell.set(true);
            false
        }
    });
    if in_hook {
        return;
    }

    if !hook_ptr.is_null() {
        let hook: unsafe extern "C" fn(*mut core::ffi::c_void, usize) =
            unsafe { core::mem::transmute(hook_ptr) };
        unsafe { hook(ptr as *mut core::ffi::c_void, size) };
    }

    if active || leak_active {
        sample_alloc_inner(ptr, size, leak_active);
    }

    #[cfg(feature = "nightly_tls")]
    unsafe {
        IN_HOOK = false;
    }
    #[cfg(not(feature = "nightly_tls"))]
    IN_HOOK.with(|cell| cell.set(false));
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

    #[cfg(feature = "nightly_tls")]
    let in_hook = unsafe {
        if IN_HOOK {
            true
        } else {
            IN_HOOK = true;
            false
        }
    };
    #[cfg(not(feature = "nightly_tls"))]
    let in_hook = IN_HOOK.with(|cell| {
        if cell.get() {
            true
        } else {
            cell.set(true);
            false
        }
    });
    if in_hook {
        return;
    }

    if !hook_ptr.is_null() {
        let hook: unsafe extern "C" fn(*mut core::ffi::c_void, usize) =
            unsafe { core::mem::transmute(hook_ptr) };
        unsafe { hook(ptr as *mut core::ffi::c_void, size) };
    }

    if active || leak_active {
        sample_free_inner(ptr);
    }

    #[cfg(feature = "nightly_tls")]
    unsafe {
        IN_HOOK = false;
    }
    #[cfg(not(feature = "nightly_tls"))]
    IN_HOOK.with(|cell| cell.set(false));
}

fn sample_alloc_inner(ptr: *mut u8, size: usize, leak_active: bool) {
    #[cfg(feature = "nightly_tls")]
    unsafe {
        let mut val = BYTES_UNTIL_SAMPLE;
        if leak_active || val <= 0 {
            if !leak_active {
                let mean = SAMPLE_INTERVAL.load(Ordering::Relaxed);
                val = next_sample_interval(mean) as isize;
            }

            let stack = capture_stack();

            let shard = (ptr as usize >> 6) % SHARDS;
            let mut lock = get_map(shard);
            if let Some(ref mut map) = *lock {
                map.insert(ptr as usize, Sample { size, stack });
            }
        }
        if !leak_active {
            BYTES_UNTIL_SAMPLE = val - size as isize;
        }
    }

    #[cfg(not(feature = "nightly_tls"))]
    BYTES_UNTIL_SAMPLE.with(|cell| {
        let mut val = cell.get();
        if leak_active || val <= 0 {
            if !leak_active {
                let mean = SAMPLE_INTERVAL.load(Ordering::Relaxed);
                val = next_sample_interval(mean) as isize;
            }

            let stack = capture_stack();

            let shard = (ptr as usize >> 6) % SHARDS;
            let mut lock = get_map(shard);
            if let Some(ref mut map) = *lock {
                map.insert(ptr as usize, Sample { size, stack });
            }
        }
        if !leak_active {
            cell.set(val - size as isize);
        }
    });
}

fn capture_stack() -> Vec<usize> {
    const MAX_STACK_FRAMES: usize = 32;
    let mut frames = [0usize; MAX_STACK_FRAMES];
    let mut len = 0usize;

    backtrace::trace(|frame| {
        let ip = frame.ip() as usize;
        if ip != 0 {
            frames[len] = ip;
            len += 1;
        }
        len < MAX_STACK_FRAMES
    });

    frames[..len].to_vec()
}

fn sample_free_inner(ptr: *mut u8) {
    let shard = (ptr as usize >> 6) % SHARDS;
    let mut lock = get_map(shard);
    if let Some(ref mut map) = *lock {
        map.remove(&(ptr as usize));
    }
}

fn next_sample_interval(mean: usize) -> usize {
    std::thread_local! {
        static RNG: core::cell::Cell<u64> = const { core::cell::Cell::new(0x123456789abcdef) };
    }
    RNG.with(|rng_state| {
        let mut state = rng_state.get();
        if state == 0 {
            state = 0x123456789abcdef
                ^ (std::time::SystemTime::now()
                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64);
        }
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        rng_state.set(state);

        let u = (state as f64) / (u64::MAX as f64);
        let u = if u < 1e-9 { 1e-9 } else { u };
        (-u.ln() * mean as f64) as usize
    })
}

/// Dumps a folded stack profile of active memory allocations to a file.
///
/// The output uses the standard collapsed stack format:
/// `func1;func2;func3 <bytes>`
pub fn dump_profile(path: &str) -> std::io::Result<()> {
    #[cfg(feature = "nightly_tls")]
    let in_hook = unsafe {
        if IN_HOOK {
            true
        } else {
            IN_HOOK = true;
            false
        }
    };
    #[cfg(not(feature = "nightly_tls"))]
    let in_hook = IN_HOOK.with(|cell| {
        if cell.get() {
            true
        } else {
            cell.set(true);
            false
        }
    });
    if in_hook {
        return Ok(());
    }

    let result = dump_profile_inner(path);

    #[cfg(feature = "nightly_tls")]
    unsafe {
        IN_HOOK = false;
    }
    #[cfg(not(feature = "nightly_tls"))]
    IN_HOOK.with(|cell| cell.set(false));
    result
}

fn dump_profile_inner(path: &str) -> std::io::Result<()> {
    let mut samples = Vec::new();
    for shard in &ACTIVE_SAMPLES {
        let lock = shard.lock().unwrap();
        if let Some(ref map) = *lock {
            for (_, sample) in map.iter() {
                samples.push(Sample {
                    size: sample.size,
                    stack: sample.stack.clone(),
                });
            }
        }
    }

    let mut folded: HashMap<String, usize> = HashMap::new();
    for sample in samples {
        let mut symbol_names = Vec::new();
        for &ip in &sample.stack {
            let mut name_opt = None;
            backtrace::resolve(ip as *mut c_void, |symbol| {
                if let Some(name) = symbol.name() {
                    name_opt = Some(name.to_string());
                }
            });
            let name_str = match name_opt {
                Some(name) => name,
                None => format!("{:#x}", ip),
            };
            symbol_names.push(name_str);
        }

        symbol_names.reverse();

        let mut filtered_symbols = Vec::new();
        for sym in symbol_names {
            if sym.contains("mnemosyne_prof")
                || sym.contains("sample_alloc")
                || sym.contains("on_alloc")
                || sym.contains("thread_alloc")
                || sym.contains("MnemosyneHeap")
                || sym.contains("backtrace::")
            {
                continue;
            }
            filtered_symbols.push(sym);
        }

        if !filtered_symbols.is_empty() {
            let stack_str = filtered_symbols.join(";");
            *folded.entry(stack_str).or_insert(0) += sample.size;
        }
    }

    let mut file = std::fs::File::create(path)?;
    for (stack, bytes) in folded {
        writeln!(file, "{} {}", stack, bytes)?;
    }

    Ok(())
}

/// Dumps all active allocations (representing leaks) with their resolved stacks to a file.
///
/// Returns the number of leaked blocks.
pub fn dump_leaks(path: &str) -> std::io::Result<usize> {
    #[cfg(feature = "nightly_tls")]
    let in_hook = unsafe {
        if IN_HOOK {
            true
        } else {
            IN_HOOK = true;
            false
        }
    };
    #[cfg(not(feature = "nightly_tls"))]
    let in_hook = IN_HOOK.with(|cell| {
        if cell.get() {
            true
        } else {
            cell.set(true);
            false
        }
    });
    if in_hook {
        return Ok(0);
    }

    let result = dump_leaks_inner(path);

    #[cfg(feature = "nightly_tls")]
    unsafe {
        IN_HOOK = false;
    }
    #[cfg(not(feature = "nightly_tls"))]
    IN_HOOK.with(|cell| cell.set(false));
    result
}

fn dump_leaks_inner(path: &str) -> std::io::Result<usize> {
    let mut samples = Vec::new();
    for shard in &ACTIVE_SAMPLES {
        let lock = shard.lock().unwrap();
        if let Some(ref map) = *lock {
            for (&ptr, sample) in map.iter() {
                samples.push((ptr, sample.size, sample.stack.clone()));
            }
        }
    }

    if samples.is_empty() {
        return Ok(0);
    }

    let mut file = std::fs::File::create(path)?;
    writeln!(file, "Mnemosyne Leak Report:")?;
    writeln!(file, "======================")?;

    let total_leaks = samples.len();
    for (ptr, size, stack) in &samples {
        writeln!(file, "\nLeak of {} bytes at {:#x}:", size, ptr)?;
        for (idx, &ip) in stack.iter().enumerate() {
            let mut name_opt = None;
            let mut filename_opt = None;
            let mut line_opt = None;
            backtrace::resolve(ip as *mut c_void, |symbol| {
                if let Some(name) = symbol.name() {
                    name_opt = Some(name.to_string());
                }
                if let Some(path_buf) = symbol.filename() {
                    filename_opt = Some(path_buf.to_string_lossy().into_owned());
                }
                if let Some(line) = symbol.lineno() {
                    line_opt = Some(line);
                }
            });

            let name = name_opt.unwrap_or_else(|| format!("{:#x}", ip));
            match (filename_opt, line_opt) {
                (Some(file_path), Some(line)) => {
                    writeln!(file, "  #{}: {} ({}:{})", idx, name, file_path, line)?;
                }
                (Some(file_path), None) => {
                    writeln!(file, "  #{}: {} ({})", idx, name, file_path)?;
                }
                _ => {
                    writeln!(file, "  #{}: {}", idx, name)?;
                }
            }
        }
    }

    Ok(total_leaks)
}

#[cfg(test)]
mod tests {
    #[test]
    fn capture_stack_stores_exact_retained_capacity() {
        let stack = super::capture_stack();
        assert!(
            stack.len() <= 32,
            "captured stack retained more than 32 frames: {}",
            stack.len()
        );
        assert_eq!(
            stack.capacity(),
            stack.len(),
            "captured stack must not retain unused frame capacity"
        );
    }
}
