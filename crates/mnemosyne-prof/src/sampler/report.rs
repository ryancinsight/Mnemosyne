use core::ffi::c_void;
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::io::Write;

use super::store::{ActiveSample, active_sample_snapshot};

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
