//! The Mnemosyne high-performance memory allocator global interface.

#![no_std]

use core::alloc::{GlobalAlloc, Layout};
use mnemosyne_core::NUM_SIZE_CLASSES;
use mnemosyne_local::{thread_alloc_layout, thread_free, thread_realloc, LocalAllocatorSelector};

pub use mnemosyne_backend::{is_cuda_available, CudaUnifiedBackend};
pub use mnemosyne_core::{options::MnemosyneOptions, AllocPolicy, StandardPolicy};
pub use mnemosyne_hardened::{HardenedPolicy, SecurePolicy};
pub use mnemosyne_heap::{
    scope as branded_scope, AllocatorToken, BrandedBlock, BrandedBox, BrandedCell, BrandedVec, Heap,
};
pub use mnemosyne_local::{usable_size, SizeClassOccupancy};
pub use mnemosyne_prof::{
    disable_leak_detector, disable_profiling, dump_leaks, dump_profile, enable_leak_detector,
    enable_profiling, is_leak_detector_enabled, is_profiling_enabled, register_alloc_hook,
    register_free_hook,
};

/// Returns the current allocator configuration options snapshot.
#[inline]
pub fn get_options() -> MnemosyneOptions {
    mnemosyne_core::options::get_options()
}
/// Configures the allocator runtime settings programmatically.
///
/// Modifies the global settings. Can be called at runtime; changes apply
/// to subsequent allocator operations. If the purge cadence is changed
/// to a non-zero value and background purger was inactive, starts the
/// background decay engine thread.
#[inline]
pub fn configure(options: MnemosyneOptions) {
    let old_cadence =
        mnemosyne_core::options::PURGE_CADENCE_MS.load(core::sync::atomic::Ordering::Acquire);
    mnemosyne_core::options::set_options(options);
    mnemosyne_local::mark_options_initialized();

    if options.purge_cadence_ms > 0 && old_cadence == 0 {
        mnemosyne_decay::init_decay_engine();
    }
}

/// Snapshot of Mnemosyne memory mapping and segment cache state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryStats {
    pub current_mapped_bytes: usize,
    pub peak_mapped_bytes: usize,
    pub map_calls: usize,
    pub unmap_calls: usize,
    /// Number of confirmed backend `page_reset` calls (Linux `MADV_DONTNEED`,
    /// macOS/FreeBSD `MADV_FREE`, Windows `VirtualAlloc(MEM_RESET)`).
    pub page_reset_calls: usize,
    /// Cumulative byte count passed to confirmed `page_reset` calls.
    pub page_reset_bytes: usize,
    /// Number of confirmed backend `make_guard` calls (Unix `mprotect(PROT_NONE)`,
    /// Windows `VirtualProtect(PAGE_NOACCESS)`).
    pub guard_install_calls: usize,
    /// Cumulative byte count passed to confirmed `make_guard` calls.
    pub guard_install_bytes: usize,
    pub retained_free_segments: usize,
    pub max_retained_free_segments: usize,
    pub retained_free_bytes: usize,
    pub purged_segments: usize,
    pub purge_calls: usize,
    pub purged_bytes: usize,
    /// Number of segments whose physical backing was released by a
    /// confirmed `page_reset` while the segment itself remained cached
    /// in the retained pool.
    pub reset_segments: usize,
    /// Number of `reset_segment_pool` invocations.
    pub reset_calls: usize,
    pub current_thread_live_allocations: usize,
    pub current_thread_owned_segments: usize,
    pub cross_thread_reclaimed_blocks: usize,
    pub page_refills: usize,
    pub recycled_pages: usize,
    pub fresh_pages: usize,
    pub fresh_segments: usize,
    pub orphan_segments_adopted: usize,
    pub recycle_sweeps: usize,
    pub size_class_occupancy: [SizeClassOccupancy; NUM_SIZE_CLASSES],
}

impl Default for MemoryStats {
    fn default() -> Self {
        Self {
            current_mapped_bytes: 0,
            peak_mapped_bytes: 0,
            map_calls: 0,
            unmap_calls: 0,
            page_reset_calls: 0,
            page_reset_bytes: 0,
            guard_install_calls: 0,
            guard_install_bytes: 0,
            retained_free_segments: 0,
            max_retained_free_segments: 0,
            retained_free_bytes: 0,
            purged_segments: 0,
            purge_calls: 0,
            purged_bytes: 0,
            reset_segments: 0,
            reset_calls: 0,
            current_thread_live_allocations: 0,
            current_thread_owned_segments: 0,
            cross_thread_reclaimed_blocks: 0,
            page_refills: 0,
            recycled_pages: 0,
            fresh_pages: 0,
            fresh_segments: 0,
            orphan_segments_adopted: 0,
            recycle_sweeps: 0,
            size_class_occupancy: [SizeClassOccupancy::default(); NUM_SIZE_CLASSES],
        }
    }
}

/// Returns current Mnemosyne allocator memory counters for a specific backend.
pub fn memory_stats_generic<B: mnemosyne_arena::HasSegmentPool + LocalAllocatorSelector<B>>(
) -> MemoryStats {
    let backend = mnemosyne_backend::backend_memory_stats();
    let arena = mnemosyne_arena::arena_memory_stats::<B>();
    let local = mnemosyne_local::thread_allocator_stats::<B>();
    MemoryStats {
        current_mapped_bytes: backend.current_mapped_bytes,
        peak_mapped_bytes: backend.peak_mapped_bytes,
        map_calls: backend.map_calls,
        unmap_calls: backend.unmap_calls,
        page_reset_calls: backend.page_reset_calls,
        page_reset_bytes: backend.page_reset_bytes,
        guard_install_calls: backend.guard_install_calls,
        guard_install_bytes: backend.guard_install_bytes,
        retained_free_segments: arena.retained_free_segments,
        max_retained_free_segments: arena.max_retained_free_segments,
        retained_free_bytes: arena.retained_free_bytes,
        purged_segments: arena.purged_segments,
        purge_calls: arena.purge_calls,
        purged_bytes: arena.purged_bytes,
        reset_segments: arena.reset_segments,
        reset_calls: arena.reset_calls,
        current_thread_live_allocations: local.current_thread_live_allocations,
        current_thread_owned_segments: local.current_thread_owned_segments,
        cross_thread_reclaimed_blocks: local.cross_thread_reclaimed_blocks,
        page_refills: local.page_refills,
        recycled_pages: local.recycled_pages,
        fresh_pages: local.fresh_pages,
        fresh_segments: local.fresh_segments,
        orphan_segments_adopted: local.orphan_segments_adopted,
        recycle_sweeps: local.recycle_sweeps,
        size_class_occupancy: local.size_class_occupancy,
    }
}

/// Returns current Mnemosyne allocator memory counters.
pub fn memory_stats() -> MemoryStats {
    memory_stats_generic::<mnemosyne_backend::MemoryBackendWrapper>()
}

/// Purges the global segment pool for a specific backend, releasing all retained/cached segments back to the OS.
pub fn purge_generic<B: mnemosyne_arena::HasSegmentPool>() {
    // Safety: Purging the segment pool releases only free segments that are
    // no longer actively referenced by any thread allocator cache.
    unsafe {
        mnemosyne_arena::purge_segment_pool::<B>();
    }
}

/// Purges the global segment pool, releasing all retained/cached segments back to the OS.
pub fn purge() {
    purge_generic::<mnemosyne_backend::MemoryBackendWrapper>();
}

/// Asks the OS to drop the physical backing of every retained free
/// segment for a specific backend without removing them from the cache.
///
/// Use this as a lighter-weight RSS-reduction knob than `purge`: the
/// segment cache stays warm so subsequent allocations skip the OS
/// mapping syscall, while the resident memory footprint of idle
/// segments drops to the kernel's demand-fault baseline.
pub fn reset_generic<B: mnemosyne_arena::HasSegmentPool>() {
    // Safety: reset_segment_pool drains the retained pool, issues
    // page_reset on each segment's mapping, and pushes them back into
    // the cache; no segment is released or accessed by another path.
    unsafe {
        mnemosyne_arena::reset_segment_pool::<B>();
    }
}

/// Asks the OS to drop the physical backing of every retained free
/// segment without removing them from the cache.
pub fn reset() {
    reset_generic::<mnemosyne_backend::MemoryBackendWrapper>();
}

/// Triggers a manual background decay and defragmentation cycle across all active memory backends.
pub fn decay() {
    mnemosyne_decay::decay_step();
}

/// The Mnemosyne global allocator structure.
///
/// Implements `core::alloc::GlobalAlloc` and routes allocations to the
/// thread-local cache or global arena.
pub struct Mnemosyne;

unsafe impl GlobalAlloc for Mnemosyne {
    // Safety: thread_alloc handles alignment constraints, size validation, and
    // OS mapping, returning null on failure or a valid memory block pointer on success.
    #[inline(always)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // `thread_alloc_layout` rejects `size == 0` through
        // `is_valid_layout_alloc_request`, so an explicit zero guard here
        // would be a redundant branch on the hottest path. The
        // single-source validation returns null for size 0, which is a
        // valid `GlobalAlloc::alloc` result.
        // Safety: size and alignment are derived from a valid Layout, and
        // the returned pointer is verified or null.
        unsafe {
            thread_alloc_layout::<StandardPolicy, mnemosyne_backend::MemoryBackendWrapper>(
                layout.size(),
                layout.align(),
            )
        }
    }

    // Safety: The ptr must be valid and previously returned by alloc.
    // thread_free determines the owner segment/page and returns blocks safely.
    #[inline(always)]
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // Safety: thread_free is safe because ptr is guaranteed by the GlobalAlloc
        // contract to be a valid pointer allocated by this allocator.
        unsafe { thread_free::<StandardPolicy, mnemosyne_backend::MemoryBackendWrapper>(ptr) }
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
>(core::marker::PhantomData<(P, B)>);

impl<P: AllocPolicy, B: mnemosyne_arena::HasSegmentPool + LocalAllocatorSelector<B>>
    MnemosyneAllocator<P, B>
{
    /// Creates a new `MnemosyneAllocator` with the specified policy and backend.
    pub const fn new() -> Self {
        Self(core::marker::PhantomData)
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
    // Safety: thread_alloc handles alignment constraints, size validation, and
    // OS mapping, returning null on failure or a valid memory block pointer on success.
    #[inline(always)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // `thread_alloc_layout` rejects `size == 0` via
        // `is_valid_layout_alloc_request`; the explicit zero guard would be
        // a redundant hot-path branch (see `Mnemosyne::alloc`).
        // Safety: size and alignment are derived from a valid Layout, and
        // the returned pointer is verified or null.
        unsafe { thread_alloc_layout::<P, B>(layout.size(), layout.align()) }
    }

    // Safety: The ptr must be valid and previously returned by alloc.
    // thread_free determines the owner segment/page and returns blocks safely.
    #[inline(always)]
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // Safety: thread_free is safe because ptr is guaranteed by the GlobalAlloc
        // contract to be a valid pointer allocated by this allocator.
        unsafe { thread_free::<P, B>(ptr) }
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
        unsafe { thread_realloc::<P, B>(ptr, layout, new_size) }
    }
}
