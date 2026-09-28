//! [`RawHeap`]'s shared struct shape, its `Send`/`Default` impls, and the
//! public `new`/`alloc`/`stats` entry points the concern submodules extend
//! through additional `impl` blocks.

use core::alloc::Layout;
use mnemosyne_core::AllocPolicy;
use mnemosyne_local::LocalAllocatorSelector;
use mnemosyne_local::internal::{HasSegmentPool, ThreadAllocator};

pub(crate) struct RawHeap<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> {
    pub(super) allocator: core::cell::UnsafeCell<ThreadAllocator<B>>,
    /// Re-entrancy gate for `allocator`, a sibling rather than a field inside
    /// it: the gate decides whether forming `&mut ThreadAllocator` is legal, so
    /// it cannot live in the memory that borrow covers. Same reasoning as
    /// `LocalAllocatorSlot::is_allocating`.
    pub(super) is_allocating: core::cell::Cell<bool>,
    _policy: core::marker::PhantomData<P>,
}

// SAFETY: `RawHeap<P, B>` holds a single `UnsafeCell<ThreadAllocator<B>>`
// and a ZST `PhantomData<P>`. `ThreadAllocator` is the per-thread
// allocator state (free lists, page lists, current segment); the
// `UnsafeCell` is what lets `&self` methods take the `&mut
// ThreadAllocator` the allocation/free paths need. `RawHeap` is not
// auto-`Send` only because `UnsafeCell<T>: !Sync` denies the auto-derive,
// not because concurrent access is sound — these methods assume exclusive
// access to the allocator and perform no internal synchronization.
//
// Cross-thread *transfer* is nonetheless sound, and required, because the
// branded wrapper `Heap<'brand, P, B>` is the only constructor and it
// confines the heap to one thread at runtime: the `'brand` invariant
// lifetime is minted exclusively through `thread_local_scope`, whose
// `ThreadLocalToken` is `!Send + !Sync`, so the heap and its token stay on
// the spawning thread for the scope's lifetime. `Send` is the necessary
// trait surface so the heap can move between threads in pathological call
// patterns; the brand mint, not this impl, is what precludes two threads
// touching the same `ThreadAllocator` concurrently. This mirrors the
// `unsafe impl Send for TieredHeap` reasoning in `tiered_heap.rs`.
unsafe impl<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Send for RawHeap<P, B> {}

impl<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> Default for RawHeap<P, B> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>> RawHeap<P, B> {
    #[inline(always)]
    pub(crate) const fn new() -> Self {
        Self {
            allocator: core::cell::UnsafeCell::new(ThreadAllocator::new()),
            is_allocating: core::cell::Cell::new(false),
            _policy: core::marker::PhantomData,
        }
    }

    #[inline(always)]
    pub(crate) fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: `alloc_inner` only requires that no other borrow of the
        // `UnsafeCell<ThreadAllocator>` is live across the call; this `&self`
        // method holds no such borrow, and thread-confinement (see the
        // `unsafe impl Send`) guarantees no concurrent accessor on another
        // thread.
        let ptr = unsafe { self.alloc_inner(layout) };
        if mnemosyne_prof::is_active() && !ptr.is_null() {
            mnemosyne_prof::on_alloc(ptr, layout.size());
        }
        ptr
    }

    #[cfg(test)]
    #[inline(always)]
    pub(crate) fn stats(&self) -> mnemosyne_local::ThreadAllocatorStats {
        // SAFETY: forms a shared `&ThreadAllocator` to read immutable
        // statistics. No `&mut` borrow of the `UnsafeCell` is live in this
        // test-only accessor, and thread-confinement precludes a concurrent
        // mutator, so the shared reference does not alias a live exclusive one.
        unsafe { (&*self.allocator.get()).stats() }
    }
}
