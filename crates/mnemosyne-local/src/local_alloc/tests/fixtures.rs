use super::super::*;
use core::sync::atomic::{AtomicUsize, Ordering};
use mnemosyne_core::MemoryBackend;

// A mock tracking memory backend to verify custom backend injection.
pub(super) struct MockBackend;
pub(super) static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
pub(super) static DEALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
pub(super) static MOCK_POOLS: mnemosyne_arena::segment::pool::BackendPools =
    mnemosyne_arena::segment::pool::BackendPools::new();

impl MemoryBackend for MockBackend {
    unsafe fn allocate(size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        // Safety: delegate to DefaultBackend
        unsafe { DefaultBackend::allocate(size) }
    }

    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        DEALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        // Safety: delegate to DefaultBackend
        unsafe { DefaultBackend::deallocate(ptr, size) }
    }
}

impl mnemosyne_arena::segment::pool::private::Sealed for MockBackend {}

impl HasSegmentPool for MockBackend {
    #[inline(always)]
    fn pools() -> &'static mnemosyne_arena::segment::pool::BackendPools {
        &MOCK_POOLS
    }
}

crate::impl_local_allocator_selector!(MockBackend);
crate::impl_local_allocator_selector!(DefaultBackend);

/// Releases every segment held by the global pools back to the OS.
///
/// Drains both backends' orphan pools — a thread that dies owning segments
/// leaves them there, and `purge_segment_pool` does not cover that pool — then
/// purges the segment and huge pools.
///
/// # Safety
///
/// No other thread may be touching the pools or any segment they hold. Callers
/// hold `TEST_LOCK`.
pub(crate) unsafe fn drain_all_pools() {
    use mnemosyne_arena::HasSegmentPool;
    // SAFETY: forwarded to the caller — the test lock excludes other threads.
    unsafe {
        while let Some(seg) = <DefaultBackend as HasSegmentPool>::global_orphan_pool().pop() {
            mnemosyne_arena::deallocate_segment::<DefaultBackend>(seg);
        }
        while let Some(seg) =
            <mnemosyne_backend::MemoryBackendWrapper as HasSegmentPool>::global_orphan_pool().pop()
        {
            mnemosyne_arena::deallocate_segment::<mnemosyne_backend::MemoryBackendWrapper>(seg);
        }
        mnemosyne_arena::purge_segment_pool::<DefaultBackend>();
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }
}

/// Drains the global pools when it goes out of scope.
///
/// Miri's leak checker runs at process exit and cannot tell a warm cache from a
/// leak, so a test that ends with segments still in the segment, huge or orphan
/// pools fails the leak gate even though retention is what those pools are for.
/// Tests that finish holding retained segments take one of these.
///
/// Declare it immediately after the `TEST_LOCK` guard: locals drop in reverse
/// order, so any `ThreadAllocator` the test builds is dropped first (returning
/// its segments to the pools), then this drains them, and only then is the lock
/// released.
pub(crate) struct PoolDrain;

impl Drop for PoolDrain {
    fn drop(&mut self) {
        // SAFETY: the test holding this also holds `TEST_LOCK`, so no other
        // thread is touching the pools.
        unsafe { drain_all_pools() };
    }
}
