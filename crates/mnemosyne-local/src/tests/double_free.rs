//! Double-free and reclaim-overflow detection: every path must abort.

use super::*;

#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn test_double_free_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_DOUBLE_FREE_ABORT_TEST").is_ok() {
        unsafe {
            let ptr = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::double_free::test_double_free_aborts_process")
        .arg("--exact")
        .env("RUN_DOUBLE_FREE_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn test_reclaim_overflow_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_RECLAIM_OVERFLOW_ABORT_TEST").is_ok() {
        unsafe {
            let ptr = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            let ptr_val = ptr as usize;
            let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;
            let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
            let page = &mut (*segment).pages[page_index];

            // Manually reduce alloc_count to 0 so count (1) > alloc_count (0) during reclaim.
            mnemosyne_core::types::Page::set_alloc_count_in_segment(segment, page_index, 0);

            // Push the block directly to the thread_free queue.
            let block = ptr as *mut Block;
            page.thread_free
                .push::<StandardPolicy>(NonNull::new_unchecked(block));

            // Run reclaim, which should detect count (1) > alloc_count (0) and abort.
            mnemosyne_core::types::Page::reclaim_thread_free_for_policy::<StandardPolicy>(
                segment, page_index,
            );
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::double_free::test_reclaim_overflow_aborts_process")
        .arg("--exact")
        .env("RUN_RECLAIM_OVERFLOW_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn test_cross_thread_double_free_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_CROSS_THREAD_DOUBLE_FREE_ABORT_TEST").is_ok() {
        let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8) };
        let ptr_val = ptr as usize;
        let handle = std::thread::spawn(move || unsafe {
            let ptr = ptr_val as *mut u8;
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
        });
        let _ = handle.join();
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::double_free::test_cross_thread_double_free_aborts_process")
        .arg("--exact")
        .env("RUN_CROSS_THREAD_DOUBLE_FREE_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn test_local_immediate_double_free_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_LOCAL_IMMEDIATE_DOUBLE_FREE_ABORT_TEST").is_ok() {
        unsafe {
            let ptr1 = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            let _ptr2 = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr1);
            thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr1);
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::double_free::test_local_immediate_double_free_aborts_process")
        .arg("--exact")
        .env("RUN_LOCAL_IMMEDIATE_DOUBLE_FREE_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn test_cpu_cache_double_free_aborts_process() {
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_CPU_CACHE_DOUBLE_FREE_ABORT_TEST").is_ok() {
        unsafe {
            crate::per_cpu::PER_CPU_CACHE_ENABLED
                .store(true, core::sync::atomic::Ordering::Relaxed);
            crate::per_cpu::enable_cpu_cache();
            let ptr = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            let ptr_val = ptr as usize;
            let handle = std::thread::spawn(move || {
                let ptr = ptr_val as *mut u8;
                thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
                thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr);
            });
            let _ = handle.join();
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::double_free::test_cpu_cache_double_free_aborts_process")
        .arg("--exact")
        .env("RUN_CPU_CACHE_DOUBLE_FREE_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}

#[test]
// Re-executes the test binary to observe the abort; Miri cannot spawn a
// subprocess, so the assertion is unobservable under it rather than failing.
#[cfg_attr(miri, ignore = "spawns a subprocess")]
fn test_thread_free_cycle_aborts_process() {
    use mnemosyne_core::policy::AllocPolicy;
    use std::env;
    use std::process::Command;
    use std::string::String;

    if env::var("RUN_THREAD_FREE_CYCLE_ABORT_TEST").is_ok() {
        unsafe {
            let ptr1 = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            let ptr2 = thread_alloc::<StandardPolicy, MemoryBackendWrapper>(16, 8);
            let ptr1_val = ptr1 as usize;
            let segment_addr = ptr1_val & !(SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;
            let page_index = (ptr1_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
            let page = &mut (*segment).pages[page_index];

            let block1 = ptr1 as *mut Block;
            let block2 = ptr2 as *mut Block;
            let cookie = if StandardPolicy::ENABLE_FREE_LIST_ENCRYPTION {
                (*segment).keys[page_index]
            } else {
                0
            };

            // Push block2 then block1 to build list block1 -> block2 -> None
            page.thread_free
                .push::<StandardPolicy>(NonNull::new_unchecked(block2));
            page.thread_free
                .push::<StandardPolicy>(NonNull::new_unchecked(block1));

            // Manually link block2 to block1 to form cycle block1 -> block2 -> block1
            (*block2).set_next::<StandardPolicy>(NonNull::new(block1), cookie);

            // Run reclaim, which should walk the cycle, detect visited (3) > count (2), and abort.
            mnemosyne_core::types::Page::reclaim_thread_free_for_policy::<StandardPolicy>(
                segment, page_index,
            );
        }
        return;
    }

    let current_exe = env::current_exe().unwrap();
    let output = Command::new(current_exe)
        .arg("tests::double_free::test_thread_free_cycle_aborts_process")
        .arg("--exact")
        .env("RUN_THREAD_FREE_CYCLE_ABORT_TEST", "1")
        .output()
        .unwrap();

    if output.status.success() {
        std::println!(
            "Subprocess stdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        );
        std::println!(
            "Subprocess stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Subprocess succeeded but was expected to abort!");
    }
}
