mod cache_aligned;
pub mod huge_pool;
pub mod list;
mod numa_bucket;
pub mod segment_pool;
mod tagged_stack;

pub use huge_pool::GlobalHugePool;
pub use list::NodeSegmentPool;
pub use segment_pool::GlobalSegmentPool;

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

static DEFAULT_BACKEND_POOLS: BackendPools = BackendPools::new();

impl private::Sealed for mnemosyne_backend::DefaultBackend {}

impl HasSegmentPool for mnemosyne_backend::DefaultBackend {
    #[inline(always)]
    fn pools() -> &'static BackendPools {
        &DEFAULT_BACKEND_POOLS
    }
}

static WRAPPER_BACKEND_POOLS: BackendPools = BackendPools::new();

impl private::Sealed for mnemosyne_backend::MemoryBackendWrapper {}

impl HasSegmentPool for mnemosyne_backend::MemoryBackendWrapper {
    #[inline(always)]
    fn pools() -> &'static BackendPools {
        &WRAPPER_BACKEND_POOLS
    }
}

static CUDA_BACKEND_POOLS: BackendPools = BackendPools::new();

impl private::Sealed for mnemosyne_backend::CudaUnifiedBackend {}

impl HasSegmentPool for mnemosyne_backend::CudaUnifiedBackend {
    #[inline(always)]
    fn pools() -> &'static BackendPools {
        &CUDA_BACKEND_POOLS
    }
}

static CUDA_DEVICE_POOLS: BackendPools = BackendPools::new();

impl private::Sealed for mnemosyne_backend::CudaDeviceBackend {}

impl HasSegmentPool for mnemosyne_backend::CudaDeviceBackend {
    #[inline(always)]
    fn pools() -> &'static BackendPools {
        &CUDA_DEVICE_POOLS
    }
}

static CUDA_HBM_POOLS: BackendPools = BackendPools::new();

impl private::Sealed for mnemosyne_backend::CudaHbmBackend {}

impl HasSegmentPool for mnemosyne_backend::CudaHbmBackend {
    #[inline(always)]
    fn pools() -> &'static BackendPools {
        &CUDA_HBM_POOLS
    }
}

static CUDA_GDDR_POOLS: BackendPools = BackendPools::new();

impl private::Sealed for mnemosyne_backend::CudaGddrBackend {}

impl HasSegmentPool for mnemosyne_backend::CudaGddrBackend {
    #[inline(always)]
    fn pools() -> &'static BackendPools {
        &CUDA_GDDR_POOLS
    }
}

static CUDA_HOST_PINNED_POOLS: BackendPools = BackendPools::new();

impl private::Sealed for mnemosyne_backend::CudaHostPinnedBackend {}

impl HasSegmentPool for mnemosyne_backend::CudaHostPinnedBackend {
    #[inline(always)]
    fn pools() -> &'static BackendPools {
        &CUDA_HOST_PINNED_POOLS
    }
}
