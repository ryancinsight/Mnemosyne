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

/// Trait associating a memory backend with its global pools.
pub trait HasSegmentPool: mnemosyne_core::MemoryBackend + private::Sealed {
    /// Returns the global segment pool for this backend.
    fn global_segment_pool() -> &'static GlobalSegmentPool;

    /// Returns the global orphan pool for this backend.
    fn global_orphan_pool() -> &'static GlobalSegmentPool;

    /// Returns the global huge allocation pool for this backend.
    fn global_huge_pool() -> &'static GlobalHugePool;
}

static DEFAULT_BACKEND_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static DEFAULT_BACKEND_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static DEFAULT_BACKEND_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();

impl private::Sealed for mnemosyne_backend::DefaultBackend {}

impl HasSegmentPool for mnemosyne_backend::DefaultBackend {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &DEFAULT_BACKEND_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &DEFAULT_BACKEND_ORPHAN_POOL
    }

    #[inline(always)]
    fn global_huge_pool() -> &'static GlobalHugePool {
        &DEFAULT_BACKEND_HUGE_POOL
    }
}

static WRAPPER_BACKEND_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static WRAPPER_BACKEND_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static WRAPPER_BACKEND_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();

impl private::Sealed for mnemosyne_backend::MemoryBackendWrapper {}

impl HasSegmentPool for mnemosyne_backend::MemoryBackendWrapper {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &WRAPPER_BACKEND_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &WRAPPER_BACKEND_ORPHAN_POOL
    }

    #[inline(always)]
    fn global_huge_pool() -> &'static GlobalHugePool {
        &WRAPPER_BACKEND_HUGE_POOL
    }
}

static CUDA_BACKEND_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static CUDA_BACKEND_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static CUDA_BACKEND_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();

impl private::Sealed for mnemosyne_backend::CudaUnifiedBackend {}

impl HasSegmentPool for mnemosyne_backend::CudaUnifiedBackend {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &CUDA_BACKEND_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &CUDA_BACKEND_ORPHAN_POOL
    }

    #[inline(always)]
    fn global_huge_pool() -> &'static GlobalHugePool {
        &CUDA_BACKEND_HUGE_POOL
    }
}

static CUDA_DEVICE_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static CUDA_DEVICE_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static CUDA_DEVICE_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();

impl private::Sealed for mnemosyne_backend::CudaDeviceBackend {}

impl HasSegmentPool for mnemosyne_backend::CudaDeviceBackend {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &CUDA_DEVICE_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &CUDA_DEVICE_ORPHAN_POOL
    }

    #[inline(always)]
    fn global_huge_pool() -> &'static GlobalHugePool {
        &CUDA_DEVICE_HUGE_POOL
    }
}

static CUDA_HOST_PINNED_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static CUDA_HOST_PINNED_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static CUDA_HOST_PINNED_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();

impl private::Sealed for mnemosyne_backend::CudaHostPinnedBackend {}

impl HasSegmentPool for mnemosyne_backend::CudaHostPinnedBackend {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &CUDA_HOST_PINNED_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &CUDA_HOST_PINNED_ORPHAN_POOL
    }

    #[inline(always)]
    fn global_huge_pool() -> &'static GlobalHugePool {
        &CUDA_HOST_PINNED_HUGE_POOL
    }
}

static WGPU_STAGING_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static WGPU_STAGING_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static WGPU_STAGING_HUGE_POOL: GlobalHugePool = GlobalHugePool::new();

impl private::Sealed for mnemosyne_backend::WgpuStagingBackend {}

impl HasSegmentPool for mnemosyne_backend::WgpuStagingBackend {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &WGPU_STAGING_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &WGPU_STAGING_ORPHAN_POOL
    }

    #[inline(always)]
    fn global_huge_pool() -> &'static GlobalHugePool {
        &WGPU_STAGING_HUGE_POOL
    }
}
