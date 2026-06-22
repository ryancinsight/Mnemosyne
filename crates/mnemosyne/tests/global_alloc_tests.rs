use core::alloc::{GlobalAlloc, Layout};
use std::thread;

use mnemosyne::{
    disable_leak_detector, dump_leaks, enable_leak_detector, is_leak_detector_enabled,
    memory_stats, purge, reset, usable_size, Mnemosyne, MnemosyneAllocator, SecurePolicy,
    StandardPolicy,
};

#[cfg(not(windows))]
use mnemosyne::{is_cuda_available, memory_stats_generic, CudaUnifiedBackend};

#[global_allocator]
static ALLOCATOR: Mnemosyne = Mnemosyne;

static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
