use core::alloc::{GlobalAlloc, Layout};
use std::thread;

use mnemosyne::{
    Mnemosyne, MnemosyneAllocator, SecurePolicy, StandardPolicy, disable_leak_detector, dump_leaks,
    enable_leak_detector, is_leak_detector_enabled, memory_stats, purge, reset, usable_size,
};

#[cfg(not(windows))]
use mnemosyne::{
    CudaDeviceBackend, CudaGddrBackend, CudaHbmBackend, CudaHostPinnedBackend, CudaUnifiedBackend,
    is_cuda_available, memory_stats_generic,
};

#[global_allocator]
static ALLOCATOR: Mnemosyne = Mnemosyne;

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquires the test serialization lock, recovering from any poison left by an
/// earlier panicking test rather than propagating it.
///
/// A poisoned lock means an earlier test panicked while holding it — not that
/// this test's fixture is unusable. Every serialized test drains the resources
/// it needs on entry, so recovering the guard lets subsequent tests report
/// their own results instead of all appearing to fail with a `PoisonError`.
fn lock_test() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[path = "global_alloc_tests/basic.rs"]
mod basic;
#[path = "global_alloc_tests/leak.rs"]
mod leak;
#[path = "global_alloc_tests/policy.rs"]
mod policy;
#[path = "global_alloc_tests/realloc.rs"]
mod realloc;
#[path = "global_alloc_tests/stats.rs"]
mod stats;
