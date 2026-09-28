//! The Mnemosyne high-performance memory allocator global interface.

#![no_std]
#![deny(missing_docs)]

extern crate alloc;

mod allocator;
mod options;
pub mod scratch;
mod stats;

pub use allocator::{Mnemosyne, MnemosyneAllocator, warm_current_thread};
pub use mnemosyne_arena::aligned_vec;
pub use mnemosyne_backend::{
    CudaDeviceBackend, CudaGddrBackend, CudaHbmBackend, CudaHostPinnedBackend, CudaUnifiedBackend,
    MemoryBackendWrapper, is_cuda_available,
};
pub use mnemosyne_core::constants::NUM_SIZE_CLASSES;
pub use mnemosyne_core::mitigations;
pub use mnemosyne_core::options::MnemosyneOptions;
pub use mnemosyne_core::size_class::{
    LEMIRE_DIV_SHIFT, SizeClassInfo, all_class_info, block_index_in_page, class_to_max_blocks,
    class_to_size, round_up_size_saturating, size_class_fragmentation, size_to_class,
    size_to_class_nonzero,
};
pub use mnemosyne_core::{AllocPolicy, HardenedPolicy, PolicyMarker, SecurePolicy, StandardPolicy};
#[cfg(feature = "branded")]
pub use mnemosyne_heap::{
    BrandedBlock, BrandedBox, BrandedCell, BrandedVec, Heap, InvariantLifetime, ReallocError,
    ReallocFailure, SyncRegionToken, ThreadLocalToken, scope as branded_scope, sync_scope,
};
pub use mnemosyne_local::{
    BinSnapshot, FastPathCacheConfig, FastPathCacheManager, FastPathEfficiencyMetrics,
    LocalAllocatorSelector, SizeClassCache, SizeClassOccupancy, all_bin_snapshots,
    alloc_distribution, bin_snapshot, flush_tls_stats, hottest_class, reset_bin_stats,
    reset_generation_count, summary_line, total_alloc_count, total_internal_fragmentation,
    total_live_bytes, total_requested_bytes, usable_size,
};
pub use mnemosyne_prof::{
    disable_leak_detector, disable_profiling, dump_leaks, dump_profile, enable_leak_detector,
    enable_profiling, is_leak_detector_enabled, is_profiling_enabled, register_alloc_hook,
    register_free_hook,
};
pub use options::{configure, get_options};
pub use scratch::{
    AlignedBuf, AlignedVec, DEFAULT_SCRATCH_ALIGN, Drain, IntoIter, MAX_POOL_SLOTS, ScratchBank,
    ScratchElement, ScratchPool,
};
pub use stats::{
    BinStatsWindow, MemoryStats, decay, memory_stats, memory_stats_generic, memory_stats_json,
    policy_summary, purge, purge_generic, purge_lazy, purge_standard, reset, reset_generic,
    top_n_classes,
};
