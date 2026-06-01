use core::ffi::c_void;
use core::sync::atomic::Ordering;
use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;

use crate::{ACTIVE_SAMPLES_COUNT, SAMPLE_INTERVAL};

/// Representation of a sampled memory allocation.
#[derive(Clone)]
pub struct Sample {
    /// Allocated size of the block in bytes.
    pub size: usize,
    /// Stack trace represented as instruction pointers.
    pub stack: Box<[usize]>,
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

/// Reset the sampler state (active samples).
pub(crate) fn reset_sampler_state() {
    for shard in &ACTIVE_SAMPLES {
        let mut lock = shard.lock().unwrap();
        *lock = None;
    }
}

pub(crate) fn sample_alloc_inner(ptr: *mut u8, size: usize, leak_active: bool) {
    #[cfg(feature = "nightly_tls")]
    {
        let mut val = crate::get_bytes_until_sample();
        if leak_active || val <= size as isize {
            if !leak_active {
                let mean = SAMPLE_INTERVAL.load(Ordering::Relaxed);
                val = next_sample_interval(mean) as isize;
            }

            let stack = capture_stack();

            let shard = (ptr as usize >> 6) % SHARDS;
            let mut lock = get_map(shard);
            if let Some(ref mut map) = *lock {
                if map.insert(ptr as usize, Sample { size, stack }).is_none() {
                    ACTIVE_SAMPLES_COUNT.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        if !leak_active {
            crate::set_bytes_until_sample(val - size as isize);
        }
    }

    #[cfg(not(feature = "nightly_tls"))]
    crate::with_bytes_until_sample(|cell| {
        let mut val = cell.get();
        if leak_active || val <= size as isize {
            if !leak_active {
                let mean = SAMPLE_INTERVAL.load(Ordering::Relaxed);
                val = next_sample_interval(mean) as isize;
            }

            let stack = capture_stack();

            let shard = (ptr as usize >> 6) % SHARDS;
            let mut lock = get_map(shard);
            if let Some(ref mut map) = *lock {
                if map.insert(ptr as usize, Sample { size, stack }).is_none() {
                    ACTIVE_SAMPLES_COUNT.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
        if !leak_active {
            cell.set(val - size as isize);
        }
    });
}

pub(crate) fn capture_stack() -> Box<[usize]> {
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

    Box::from(&frames[..len])
}

pub(crate) fn sample_free_inner(ptr: *mut u8) {
    let shard = (ptr as usize >> 6) % SHARDS;
    let mut lock = get_map(shard);
    if let Some(ref mut map) = *lock {
        if map.remove(&(ptr as usize)).is_some() {
            ACTIVE_SAMPLES_COUNT.fetch_sub(1, Ordering::Relaxed);
        }
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
    let in_hook = crate::enter_hook();
    if in_hook {
        return Ok(());
    }

    let result = dump_profile_inner(path);

    crate::exit_hook();
    result
}

fn dump_profile_inner(path: &str) -> std::io::Result<()> {
    let mut samples = Vec::new();
    for shard in &ACTIVE_SAMPLES {
        let lock = shard.lock().unwrap();
        if let Some(ref map) = *lock {
            for (_, sample) in map.iter() {
                samples.push(sample.clone());
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
    let in_hook = crate::enter_hook();
    if in_hook {
        return Ok(0);
    }

    let result = dump_leaks_inner(path);

    crate::exit_hook();
    result
}

fn dump_leaks_inner(path: &str) -> std::io::Result<usize> {
    let mut samples = Vec::new();
    for shard in &ACTIVE_SAMPLES {
        let lock = shard.lock().unwrap();
        if let Some(ref map) = *lock {
            for (&ptr, sample) in map.iter() {
                samples.push((ptr, sample.clone()));
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
    for (ptr, sample) in &samples {
        writeln!(file, "\nLeak of {} bytes at {:#x}:", sample.size, ptr)?;
        for (idx, &ip) in sample.stack.iter().enumerate() {
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
