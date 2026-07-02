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
