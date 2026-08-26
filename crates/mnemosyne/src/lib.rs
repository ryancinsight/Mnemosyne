//! The Mnemosyne high-performance memory allocator global interface.

#![no_std]
#![deny(missing_docs)]

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
    AllocPolicy, HardenedPolicy, SecurePolicy, StandardPolicy, options::MnemosyneOptions,
};
#[cfg(feature = "branded")]
pub use mnemosyne_heap::{
    BrandedBlock, BrandedBox, BrandedCell, BrandedVec, Heap, InvariantLifetime, ReallocError,
    ReallocFailure, ThreadLocalToken, scope as branded_scope,
};
pub use mnemosyne_local::{
    FastPathCacheConfig, FastPathCacheManager, FastPathEfficiencyMetrics, LocalAllocatorSelector,
    SizeClassCache, SizeClassOccupancy, usable_size,
};
pub use mnemosyne_prof::{
    disable_leak_detector, disable_profiling, dump_leaks, dump_profile, enable_leak_detector,
    enable_profiling, is_leak_detector_enabled, is_profiling_enabled, register_alloc_hook,
    register_free_hook,
};
pub use options::{configure, get_options};
pub use stats::{
    MemoryStats, decay, memory_stats, memory_stats_generic, purge, purge_generic, reset,
    reset_generic,
};
