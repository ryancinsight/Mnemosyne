mod capture;
mod hasher;
mod stack_interner;

use core::ffi::c_void;
use core::sync::atomic::Ordering;
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::sync::{Arc, Mutex};

use crate::{ACTIVE_SAMPLES_COUNT, SAMPLE_INTERVAL};

use capture::{capture_stack, next_sample_interval};
use hasher::FastBuildHasher;
pub use stack_interner::StackId;
use stack_interner::{release_stack, reset_stack_interner_state, resolve_stack};

/// Representation of a sampled memory allocation.
#[derive(Clone, Copy)]
pub struct Sample {
    /// Allocated size of the block in bytes.
    pub size: usize,
    /// Interned identity of the retained stack trace (resolve via the interner).
    pub stack: StackId,
}

#[derive(Clone)]
struct ActiveSample {
    ptr: usize,
    size: usize,
    stack: Arc<[usize]>,
}

const SHARDS: usize = 64;

#[repr(align(64))]
struct Shard {
    mutex: Mutex<Option<HashMap<usize, Sample, FastBuildHasher>>>,
}

static ACTIVE_SAMPLES: [Shard; SHARDS] = [const {
    Shard {
        mutex: Mutex::new(None),
    }
}; SHARDS];

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
    reset_stack_interner_state();
}

pub(crate) fn sample_alloc_inner(ptr: *mut u8, size: usize, leak_active: bool) {
    let debit = crate::sample_debit(size);

    #[cfg(nightly_tls_active)]
    {
        let mut val = crate::get_bytes_until_sample();
        maybe_record_sample(ptr, size, leak_active, debit, &mut val);
        crate::set_bytes_until_sample(val);
    }

    // SAFETY: `get_profiler_state()` returns this thread's own thread-local
    // `ThreadState`; the `&mut` is exclusive (thread-local) and this runs inside
    // the `enter_hook`/`exit_hook` re-entrancy guard, so no nested `&mut` to the
    // same state can be live.
    #[cfg(not(nightly_tls_active))]
    unsafe {
        let state = &mut *crate::get_profiler_state();
        maybe_record_sample(ptr, size, leak_active, debit, &mut state.bytes_until_sample);
    }
}

fn maybe_record_sample(
    ptr: *mut u8,
    size: usize,
    leak_active: bool,
    debit: isize,
    bytes_until_sample: &mut isize,
) {
    if leak_active || *bytes_until_sample <= debit {
        if !leak_active {
            let mean = SAMPLE_INTERVAL.load(Ordering::Relaxed);
            *bytes_until_sample = next_sample_interval(mean) as isize;
        }

        let stack = capture_stack();
        let shard = sample_shard(ptr as usize);
        let replaced = {
            let mut lock = get_map(shard);
            if let Some(ref mut map) = *lock {
                let replaced = map.insert(ptr as usize, Sample { size, stack });
                if replaced.is_none() {
                    ACTIVE_SAMPLES_COUNT.fetch_add(1, Ordering::Relaxed);
                }
                replaced
            } else {
                None
            }
        };
        if let Some(replaced) = replaced {
            release_stack(replaced.stack);
        }
    }

    if !leak_active {
        *bytes_until_sample = (*bytes_until_sample).saturating_sub(debit);
    }
}

pub(crate) fn sample_free_inner(ptr: *mut u8) {
    let shard = sample_shard(ptr as usize);
    let removed = {
        let mut lock = get_map(shard);
        if let Some(ref mut map) = *lock {
            let removed = map.remove(&(ptr as usize));
            if removed.is_some() {
                ACTIVE_SAMPLES_COUNT.fetch_sub(1, Ordering::Relaxed);
            }
            removed
        } else {
            None
        }
    };
    if let Some(sample) = removed {
        release_stack(sample.stack);
    }
}

#[inline]
fn sample_shard(ptr: usize) -> usize {
    (ptr >> 6) % SHARDS
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
    let samples = active_sample_snapshot();
    for sample in &samples {
        fold_sample_stack(sample, &mut folded);
    }

    let mut file = std::fs::File::create(path)?;
    for (stack, bytes) in folded {
        writeln!(file, "{} {}", stack, bytes)?;
    }

    Ok(())
}

fn fold_sample_stack(sample: &ActiveSample, folded: &mut HashMap<String, usize>) {
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
    let samples = active_sample_snapshot();
    for sample in &samples {
        let file = leak_report_file(path, &mut file)?;
        write_leak_sample(file, sample)?;
    }

    Ok(samples.len())
}

fn active_sample_snapshot() -> Vec<ActiveSample> {
    let total = ACTIVE_SAMPLES_COUNT.load(Ordering::Relaxed);
    let mut samples = Vec::with_capacity(total);
    for shard in &ACTIVE_SAMPLES {
        let lock = shard
            .mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(ref map) = *lock {
            samples.extend(map.iter().filter_map(|(&ptr, sample)| {
                resolve_stack(sample.stack).map(|stack| ActiveSample {
                    ptr,
                    size: sample.size,
                    stack,
                })
            }));
        }
    }
    samples
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

fn write_leak_sample(file: &mut std::fs::File, sample: &ActiveSample) -> std::io::Result<()> {
    writeln!(
        file,
        "\nLeak of {} bytes at {:#x}:",
        sample.size, sample.ptr
    )?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_sample_snapshot_is_detached_from_live_shards() {
        crate::reset_profiler_for_testing();

        let ptr = 0x1000usize as *mut u8;
        sample_alloc_inner(ptr, 64, true);

        let snapshot = active_sample_snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].ptr, ptr as usize);
        assert_eq!(snapshot[0].size, 64);
        assert!(
            !snapshot[0].stack.is_empty(),
            "snapshot must retain resolved stack frames"
        );

        sample_free_inner(ptr);
        assert!(
            active_sample_snapshot().is_empty(),
            "live shard map must be empty after freeing the sampled pointer"
        );
        assert_eq!(
            snapshot[0].size, 64,
            "snapshot must retain a value copy after the live sample is removed"
        );

        crate::reset_profiler_for_testing();
    }
}
