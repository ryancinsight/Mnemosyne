//! Global segment caches: per-NUMA-node pools of retained segments and huge
//! mappings, with cross-node stealing when the local node is empty.

mod cache_aligned;
pub mod huge_pool;
pub mod list;
mod node_huge_bucket;
mod numa_bucket;
pub mod segment_pool;
mod tagged_stack;

pub use huge_pool::GlobalHugePool;
pub use list::NodeSegmentPool;
pub use segment_pool::GlobalSegmentPool;

/// Point-in-time snapshot of [`GlobalHugePool`] counters.
///
/// All fields are individually relaxed reads; not jointly consistent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HugePoolStats {
    /// Huge blocks currently held in the warm cache.
    pub retained_blocks: usize,
    /// Total bytes of huge blocks in the warm cache.
    pub retained_bytes: usize,
}

/// Point-in-time snapshot of [`GlobalSegmentPool`] telemetry counters.
///
/// All fields are individually monotone-non-decreasing relaxed reads;
/// they are not jointly consistent (no single atomic snapshot).
///
/// `#[non_exhaustive]`: this snapshot has gained fields three times
/// (`reset_segments`/`reset_calls`, `oom_retries`/`oom_retry_successes`) as
/// telemetry grew, and will again — a growing counter set is exactly the
/// forward-compatibility case the attribute exists for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub struct SegmentPoolStats {
    /// Segments currently held in the warm cache.
    pub retained: usize,
    /// Cumulative segments returned to the OS by purge passes.
    pub purged_segments: usize,
    /// Cumulative purge-pass invocations.
    pub purge_calls: usize,
    /// Cumulative segments whose physical backing was dropped by a page-reset.
    pub reset_segments: usize,
    /// Cumulative reset-pass invocations.
    pub reset_calls: usize,
    /// Cumulative purge-and-retry attempts after a first OS allocation
    /// failure (`allocate_segment`'s OOM recovery path).
    pub oom_retries: usize,
    /// Cumulative purge-and-retry attempts whose retried allocation
    /// succeeded, a subset of `oom_retries`.
    pub oom_retry_successes: usize,
}

/// Sealed trait module to protect architectural invariants.
#[doc(hidden)]
pub mod private {
    pub trait Sealed {}
}

/// The trio of global pools owned by a single memory backend.
///
/// One `const`-constructible bundle replaces the per-backend triplet of
/// separate statics: each backend owns exactly one `static BackendPools`,
/// and [`HasSegmentPool`] exposes its three components through default
/// accessor methods.
pub struct BackendPools {
    segment: GlobalSegmentPool,
    orphan: GlobalSegmentPool,
    huge: GlobalHugePool,
}

impl BackendPools {
    /// Creates a bundle of three empty pools.
    ///
    /// Const-constructible so each backend can declare its pools as a single
    /// `static`, preserving the distinct-per-backend isolation that separate
    /// statics previously provided.
    pub const fn new() -> Self {
        Self {
            segment: GlobalSegmentPool::new(),
            orphan: GlobalSegmentPool::new(),
            huge: GlobalHugePool::new(),
        }
    }
}

impl Default for BackendPools {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Implements [`private::Sealed`] and [`HasSegmentPool`] for a backend type,
/// associating it with a named `static BackendPools` instance.
///
/// Each backend requires exactly 5 boilerplate lines; this macro eliminates
/// the duplication across all 7 backends (35 → 7 lines + this definition).
macro_rules! impl_has_segment_pool {
    ($backend:ty, $pools_name:ident) => {
        static $pools_name: BackendPools = BackendPools::new();
        impl private::Sealed for $backend {}
        impl HasSegmentPool for $backend {
            #[inline(always)]
            fn pools() -> &'static BackendPools {
                &$pools_name
            }
        }
    };
}

/// Trait associating a memory backend with its global pools.
///
/// Implementors provide a single [`HasSegmentPool::pools`] accessor returning
/// their owned [`BackendPools`]; the individual pool accessors are supplied as
/// default methods delegating to it.
pub trait HasSegmentPool: mnemosyne_core::MemoryBackend + private::Sealed {
    /// Returns this backend's pool bundle.
    fn pools() -> &'static BackendPools;

    /// Returns the global segment pool for this backend.
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &Self::pools().segment
    }

    /// Returns the global orphan pool for this backend.
    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &Self::pools().orphan
    }

    /// Returns the global huge allocation pool for this backend.
    #[inline(always)]
    fn global_huge_pool() -> &'static GlobalHugePool {
        &Self::pools().huge
    }
}

impl_has_segment_pool!(mnemosyne_backend::DefaultBackend, DEFAULT_BACKEND_POOLS);
impl_has_segment_pool!(
    mnemosyne_backend::MemoryBackendWrapper,
    WRAPPER_BACKEND_POOLS
);
impl_has_segment_pool!(mnemosyne_backend::CudaUnifiedBackend, CUDA_BACKEND_POOLS);
impl_has_segment_pool!(mnemosyne_backend::CudaDeviceBackend, CUDA_DEVICE_POOLS);
impl_has_segment_pool!(mnemosyne_backend::CudaHbmBackend, CUDA_HBM_POOLS);
impl_has_segment_pool!(mnemosyne_backend::CudaGddrBackend, CUDA_GDDR_POOLS);
impl_has_segment_pool!(
    mnemosyne_backend::CudaHostPinnedBackend,
    CUDA_HOST_PINNED_POOLS
);
