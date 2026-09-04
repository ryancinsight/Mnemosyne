//! The Mnemosyne high-performance memory allocator global interface.

#![no_std]
#![deny(missing_docs)]

extern crate alloc;

mod allocator;
mod options;
pub mod scratch;
mod stats;

pub use allocator::{Mnemosyne, MnemosyneAllocator};
pub use mnemosyne_backend::{
    CudaDeviceBackend, CudaGddrBackend, CudaHbmBackend, CudaHostPinnedBackend, CudaUnifiedBackend,
    MemoryBackendWrapper, is_cuda_available,
};
pub use mnemosyne_core::{
    AllocPolicy, HardenedPolicy, PolicyMarker, SecurePolicy, StandardPolicy,
    constants::NUM_SIZE_CLASSES,
    mitigations,
    options::MnemosyneOptions,
    size_class::{
        LEMIRE_DIV_SHIFT, block_index_in_page, class_to_max_blocks, class_to_size, size_to_class,
        size_to_class_nonzero,
    },
};
#[cfg(feature = "branded")]
pub use mnemosyne_heap::{
    BrandedBlock, BrandedBox, BrandedCell, BrandedVec, Heap, InvariantLifetime, ReallocError,
    ReallocFailure, SyncRegionToken, ThreadLocalToken, scope as branded_scope, sync_scope,
};
pub use mnemosyne_local::{
    BinSnapshot, FastPathCacheConfig, FastPathCacheManager, FastPathEfficiencyMetrics,
    LocalAllocatorSelector, SizeClassCache, SizeClassOccupancy, all_bin_snapshots, bin_snapshot,
    flush_tls_stats, hottest_class, reset_bin_stats, summary_line, total_alloc_count,
    total_live_bytes, usable_size,
};
pub use mnemosyne_prof::{
    disable_leak_detector, disable_profiling, dump_leaks, dump_profile, enable_leak_detector,
    enable_profiling, is_leak_detector_enabled, is_profiling_enabled, register_alloc_hook,
    register_free_hook,
};
pub use options::{configure, get_options};
pub use scratch::{AlignedVec, Drain, IntoIter};
pub use stats::{
    MemoryStats, decay, memory_stats, memory_stats_generic, memory_stats_json, purge,
    purge_generic, purge_lazy, reset, reset_generic,
};

/// Forces the Mnemosyne thread-local allocator to initialize for the current
/// thread by performing a minimal allocation and deallocation through the
/// `Mnemosyne` allocator.
///
/// Call this before starting any measurement window (e.g., a
/// `stats_alloc::Region`) to flush thread-local-state initialization traffic
/// — options parsing, arena segment acquisition, per-thread allocator setup —
/// out of the window so that only the actual code under test is measured.
///
/// # Example
///
/// ```rust,no_run
/// # use mnemosyne::warm_current_thread;
/// warm_current_thread();
/// // ... zero-allocation work; the warm call is outside the measure window
/// ```
pub fn warm_current_thread() {
    use core::alloc::{GlobalAlloc, Layout};
    let layout = Layout::new::<[u8; 8]>();
    // SAFETY: `layout` is a valid non-zero `Layout` for eight bytes; the
    // returned pointer is null-checked before deallocation.
    let ptr = unsafe { Mnemosyne.alloc(layout) };
    if !ptr.is_null() {
        // SAFETY: `ptr` is a valid allocation from the `Mnemosyne` allocator
        // with `layout`, and is freed exactly once immediately here.
        unsafe { Mnemosyne.dealloc(ptr, layout) };
    }
}
