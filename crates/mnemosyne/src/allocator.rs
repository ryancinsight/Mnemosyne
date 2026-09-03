use core::alloc::{GlobalAlloc, Layout};
use core::marker::PhantomData;

use mnemosyne_local::{thread_alloc_layout, thread_free_layout, thread_realloc};

use crate::{AllocPolicy, LocalAllocatorSelector, StandardPolicy};

/// The Mnemosyne global allocator structure.
///
/// Implements `core::alloc::GlobalAlloc` and routes allocations to the
/// thread-local cache or global arena.
pub struct Mnemosyne;

unsafe impl GlobalAlloc for Mnemosyne {
    // SAFETY: thread_alloc handles alignment constraints, size validation, and
    // OS mapping, returning null on failure or a valid memory block pointer on success.
    #[inline(always)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // `thread_alloc_layout` rejects `size == 0` through
        // `is_valid_layout_alloc_request`, so an explicit zero guard here
        // would be a redundant branch on the hottest path. The
        // single-source validation returns null for size 0, which is a
        // valid `GlobalAlloc::alloc` result.
        // SAFETY: size and alignment are derived from a valid Layout, and
        // the returned pointer is verified or null.
        unsafe {
            thread_alloc_layout::<StandardPolicy, mnemosyne_backend::MemoryBackendWrapper>(
                layout.size(),
                layout.align(),
            )
        }
    }

    // SAFETY: The ptr must be valid and previously returned by alloc.
    // thread_free determines the owner segment/page and returns blocks safely.
    #[inline(always)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: thread_free is safe because ptr is guaranteed by the GlobalAlloc
        // contract to be a valid pointer allocated by this allocator.
        unsafe {
            thread_free_layout::<StandardPolicy, mnemosyne_backend::MemoryBackendWrapper>(
                ptr,
                layout.size(),
                layout.align(),
            )
        }
    }

    /// In-place `realloc` shortcut for within-class size changes.
    ///
    /// When the new size fits inside the size-class block already
    /// reserved for `ptr`, return `ptr` unchanged — the allocation
    /// already covers the request. This eliminates the alloc/copy/free
    /// round trip that the default `GlobalAlloc::realloc` performs and
    /// is the common case for `Vec<T>::push` capacity-rounding because
    /// Mnemosyne rounds small requests up to the next size class.
    ///
    /// Falls through to the default `alloc + copy + dealloc` path when:
    ///   - `ptr` is null (treated as a fresh allocation),
    ///   - `new_size` is 0 (treated as a deallocation),
    ///   - `new_size` exceeds the current usable size and a new size
    ///     class is required,
    ///   - `new_size` is less than 50% of the current size (capacity-shrink
    ///     heuristic), forcing a real shrink to release memory.
    ///
    /// # Safety
    ///
    /// `ptr` must be a previously-returned Mnemosyne allocation with
    /// the given `layout`; `new_size` must be a valid `Layout` size
    /// when paired with `layout.align()`. Same contract as the default
    /// `GlobalAlloc::realloc`.
    #[inline(always)]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `GlobalAlloc::realloc` supplies the same live-pointer and
        // `Layout` invariants that `thread_realloc` requires.
        unsafe {
            thread_realloc::<StandardPolicy, mnemosyne_backend::MemoryBackendWrapper>(
                ptr, layout, new_size,
            )
        }
    }
}

/// Generic global allocator that is parameterized by an allocation policy `P` and a memory backend `B`.
///
/// This permits zero-cost compile-time configuration of allocator behaviors
/// (e.g. `SecurePolicy` for memory zeroing and poisoning) and backends (e.g. `CudaUnifiedBackend`).
pub struct MnemosyneAllocator<
    P: AllocPolicy,
    B: mnemosyne_arena::HasSegmentPool + LocalAllocatorSelector<B> = mnemosyne_backend::MemoryBackendWrapper,
>(PhantomData<(P, B)>);

impl<P: AllocPolicy, B: mnemosyne_arena::HasSegmentPool + LocalAllocatorSelector<B>>
    MnemosyneAllocator<P, B>
{
    /// Creates a new `MnemosyneAllocator` with the specified policy and backend.
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<P: AllocPolicy, B: mnemosyne_arena::HasSegmentPool + LocalAllocatorSelector<B>> Default
    for MnemosyneAllocator<P, B>
{
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl<P: AllocPolicy, B: mnemosyne_arena::HasSegmentPool + LocalAllocatorSelector<B>>
    GlobalAlloc for MnemosyneAllocator<P, B>
{
    // SAFETY: thread_alloc handles alignment constraints, size validation, and
    // OS mapping, returning null on failure or a valid memory block pointer on success.
    #[inline(always)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // `thread_alloc_layout` rejects `size == 0` via
        // `is_valid_layout_alloc_request`; the explicit zero guard would be
        // a redundant hot-path branch (see `Mnemosyne::alloc`).
        // SAFETY: size and alignment are derived from a valid Layout, and
        // the returned pointer is verified or null.
        unsafe { thread_alloc_layout::<P, B>(layout.size(), layout.align()) }
    }

    // SAFETY: The ptr must be valid and previously returned by alloc.
    // thread_free determines the owner segment/page and returns blocks safely.
    #[inline(always)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: thread_free is safe because ptr is guaranteed by the GlobalAlloc
        // contract to be a valid pointer allocated by this allocator.
        unsafe { thread_free_layout::<P, B>(ptr, layout.size(), layout.align()) }
    }

    /// In-place `realloc` shortcut. See `Mnemosyne::realloc` for the
    /// full rationale (including capacity-shrink heuristic details); the
    /// generic variant uses the policy-aware `thread_alloc_layout` and
    /// `thread_free` paths so a `SecurePolicy` realloc still zeroes/poisons
    /// the slow-path replacement.
    ///
    /// # Safety
    ///
    /// Same contract as `Mnemosyne::realloc`.
    #[inline(always)]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the `GlobalAlloc::realloc` caller provides a live allocation
        // and matching `Layout`; this forwards those exact invariants to the
        // policy/backend-specific realloc path.
        unsafe { thread_realloc::<P, B>(ptr, layout, new_size) }
    }
}
