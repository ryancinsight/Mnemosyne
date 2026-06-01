use crate::raw_heap::RawHeap;
use core::alloc::Layout;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::internal::HasSegmentPool;

/// An explicit custom memory heap.
///
/// Threads can instantiate a `MnemosyneHeap` to manage their own isolated allocation stream.
/// When the heap is dropped, all segments owned by it are automatically reclaimed or orphaned.
pub struct MnemosyneHeap<P: AllocPolicy, B: HasSegmentPool = mnemosyne_backend::DefaultBackend> {
    raw: RawHeap<P, B>,
}

unsafe impl<P: AllocPolicy, B: HasSegmentPool> Send for MnemosyneHeap<P, B> {}

impl<P: AllocPolicy, B: HasSegmentPool> Default for MnemosyneHeap<P, B> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<P: AllocPolicy, B: HasSegmentPool> MnemosyneHeap<P, B> {
    /// Creates a new empty `MnemosyneHeap`.
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            raw: RawHeap::new(),
        }
    }

    /// Allocates a block of memory from this heap.
    ///
    /// Returns null if allocation fails.
    #[inline(always)]
    pub fn alloc(&self, layout: Layout) -> *mut u8 {
        self.raw.alloc(layout)
    }

    /// Frees a block of memory back to its originating heap/allocator.
    ///
    /// # Safety
    ///
    /// The pointer must be null or previously allocated by this heap.
    #[inline(always)]
    pub unsafe fn free(&self, ptr: *mut u8) {
        unsafe { self.raw.free(ptr) };
    }

    /// Reallocates a memory block from this heap.
    ///
    /// # Safety
    ///
    /// The pointer must be null or previously allocated by this heap.
    #[inline(always)]
    pub unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { self.raw.realloc(ptr, layout, new_size) }
    }
}
