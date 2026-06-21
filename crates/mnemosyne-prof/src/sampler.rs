use core::ffi::c_void;
use core::sync::atomic::Ordering;
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::sync::Mutex;

use crate::{ACTIVE_SAMPLES_COUNT, SAMPLE_INTERVAL};

use core::hash::{BuildHasher, Hasher};

#[derive(Default, Clone, Copy)]
pub struct FastHasher(u64);

impl Hasher for FastHasher {
    #[inline(always)]
    fn finish(&self) -> u64 {
        self.0
    }

    #[inline(always)]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = self.0.wrapping_mul(109) ^ (byte as u64);
        }
    }

    #[inline(always)]
    fn write_u8(&mut self, i: u8) {
        self.0 = self.0.wrapping_mul(109) ^ (i as u64);
    }

    #[inline(always)]
    fn write_usize(&mut self, i: usize) {
        let mut x = i as u64;
        x ^= x >> 30;
        x = x.wrapping_mul(0xbf58476d1ce4e5b9);
        x ^= x >> 27;
        x = x.wrapping_mul(0x94d049bb133111eb);
        x ^= x >> 31;
        self.0 = x;
    }
}

#[derive(Default, Clone, Copy)]
pub struct FastBuildHasher;

impl BuildHasher for FastBuildHasher {
    type Hasher = FastHasher;

    #[inline(always)]
    fn build_hasher(&self) -> Self::Hasher {
        FastHasher(0)
    }
}

/// Representation of a sampled memory allocation.
#[derive(Clone)]
pub struct Sample {
    /// Allocated size of the block in bytes.
    pub size: usize,
    /// Exact retained stack trace represented as instruction pointers.
    pub stack: Box<[usize]>,
}

const SHARDS: usize = 64;

#[repr(align(64))]
struct Shard {
    mutex: Mutex<Option<HashMap<usize, Sample, FastBuildHasher>>>,
}

static ACTIVE_SAMPLES: [Shard; SHARDS] =
    [const { Shard { mutex: Mutex::new(None) } }; SHARDS];

fn get_map(
    shard: usize,
) -> std::sync::MutexGuard<'static, Option<HashMap<usize, Sample, FastBuildHasher>>> {
    let mut lock = ACTIVE_SAMPLES[shard]
        .mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if lock.is_none() {
        *lock = Some(HashMap::with_hasher(FastBuildHasher));
    }
    lock
}

/// Reset the sampler state (active samples).
pub(crate) fn reset_sampler_state() {
    for shard in &ACTIVE_SAMPLES {
        let mut lock = shard
            .mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *lock = None;
    }
}

pub(crate) fn sample_alloc_inner(ptr: *mut u8, size: usize, leak_active: bool) {
    #[cfg(nightly_tls_active)]
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

    #[cfg(not(nightly_tls_active))]
    unsafe {
        let state = &mut *crate::get_profiler_state();
        let mut val = state.bytes_until_sample;
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
            state.bytes_until_sample = val - size as isize;
        }
    }
}

pub(crate) fn capture_stack() -> Box<[usize]> {
    const MAX_STACK_FRAMES: usize = 32;
    let mut frames = [0usize; MAX_STACK_FRAMES];
    let mut len = 0usize;

    backtrace::trace(|frame| {
        let ip = frame.ip() as usize;
        if ip != 0 {
            if len == MAX_STACK_FRAMES {
                return false;
            }
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
    let mut folded: HashMap<String, usize> = HashMap::new();
    for shard in &ACTIVE_SAMPLES {
        let lock = shard
            .mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(ref map) = *lock {
            for (_, sample) in map.iter() {
                fold_sample_stack(sample, &mut folded);
            }
        }
    }

    let mut file = std::fs::File::create(path)?;
    for (stack, bytes) in folded {
        writeln!(file, "{} {}", stack, bytes)?;
    }

    Ok(())
}

fn fold_sample_stack(sample: &Sample, folded: &mut HashMap<String, usize>) {
    let mut stack = String::new();
    for &ip in sample.stack.iter().rev() {
        let mut symbol = String::new();
        backtrace::resolve(ip as *mut c_void, |resolved| {
            if let Some(name) = resolved.name() {
                let _ = write!(symbol, "{name}");
            }
        });
        if symbol.is_empty() {
            let _ = write!(symbol, "{ip:#x}");
        }
        if is_profiler_internal_symbol(&symbol) {
            continue;
        }
        if !stack.is_empty() {
            stack.push(';');
        }
        stack.push_str(&symbol);
    }

    if !stack.is_empty() {
        *folded.entry(stack).or_insert(0) += sample.size;
    }
}

#[inline]
fn is_profiler_internal_symbol(symbol: &str) -> bool {
    symbol.contains("mnemosyne_prof")
        || symbol.contains("sample_alloc")
        || symbol.contains("on_alloc")
        || symbol.contains("thread_alloc")
        || symbol.contains("mnemosyne_heap::heap::Heap")
        || symbol.contains("backtrace::")
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
    let mut file = None;
    let mut total_leaks = 0usize;
    for shard in &ACTIVE_SAMPLES {
        let lock = shard
            .mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(ref map) = *lock {
            for (&ptr, sample) in map.iter() {
                let file = leak_report_file(path, &mut file)?;
                write_leak_sample(file, ptr, sample)?;
                total_leaks += 1;
            }
        }
    }

    Ok(total_leaks)
}

fn leak_report_file<'a>(
    path: &str,
    file: &'a mut Option<std::fs::File>,
) -> std::io::Result<&'a mut std::fs::File> {
    if file.is_none() {
        let mut created = std::fs::File::create(path)?;
        write_leak_report_header(&mut created)?;
        *file = Some(created);
    }
    match file.as_mut() {
        Some(file) => Ok(file),
        None => Err(std::io::Error::other(
            "leak report file was not initialized",
        )),
    }
}

fn write_leak_report_header(file: &mut std::fs::File) -> std::io::Result<()> {
    writeln!(file, "Mnemosyne Leak Report:")?;
    writeln!(file, "======================")
}

fn write_leak_sample(file: &mut std::fs::File, ptr: usize, sample: &Sample) -> std::io::Result<()> {
    writeln!(file, "\nLeak of {} bytes at {:#x}:", sample.size, ptr)?;
    for (idx, &ip) in sample.stack.iter().enumerate() {
        let mut name = String::new();
        let mut file_path = String::new();
        let mut line_opt = None;
        backtrace::resolve(ip as *mut c_void, |symbol| {
            if let Some(symbol_name) = symbol.name() {
                let _ = write!(name, "{symbol_name}");
            }
            if let Some(path_buf) = symbol.filename() {
                let _ = write!(file_path, "{}", path_buf.to_string_lossy());
            }
            line_opt = symbol.lineno();
        });
        if name.is_empty() {
            let _ = write!(name, "{ip:#x}");
        }

        match (file_path.is_empty(), line_opt) {
            (false, Some(line)) => writeln!(file, "  #{}: {} ({}:{})", idx, name, file_path, line)?,
            (false, None) => writeln!(file, "  #{}: {} ({})", idx, name, file_path)?,
            (true, _) => writeln!(file, "  #{}: {}", idx, name)?,
        }
    }
    Ok(())
}
