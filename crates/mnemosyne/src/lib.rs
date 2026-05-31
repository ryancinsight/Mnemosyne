//! The Mnemosyne high-performance memory allocator global interface.

#![no_std]

use core::alloc::{GlobalAlloc, Layout};
use mnemosyne_core::NUM_SIZE_CLASSES;
use mnemosyne_local::{thread_alloc_layout, thread_free, thread_realloc, LocalAllocatorSelector};

pub use mnemosyne_backend::{is_cuda_available, CudaUnifiedBackend};
pub use mnemosyne_core::{AllocPolicy, StandardPolicy};
pub use mnemosyne_hardened::{HardenedPolicy, SecurePolicy};
pub use mnemosyne_local::{usable_size, SizeClassOccupancy};
pub use mnemosyne_heap::{
    scope as branded_scope, AllocatorToken, BrandedBlock, BrandedBox, BrandedCell, BrandedHeap,
    BrandedVec, MnemosyneHeap,
};
pub use mnemosyne_prof::{
    register_alloc_hook, register_free_hook, enable_profiling, disable_profiling,
    is_profiling_enabled, dump_profile,
};

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

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::thread;

    #[global_allocator]
    static ALLOCATOR: Mnemosyne = Mnemosyne;

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_basic_allocation() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        let x = std::boxed::Box::new(42);
        assert_eq!(*x, 42);
        drop(x);
    }

    #[test]
    fn test_multithreaded_allocation() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        let mut handles = std::vec::Vec::new();
        for _ in 0..10 {
            handles.push(thread::spawn(|| {
                for _ in 0..100 {
                    let mut v = std::vec::Vec::new();
                    for i in 0..100 {
                        v.push(i);
                    }
                    assert_eq!(v[50], 50);
                }
            }));
        }
        for handle in handles {
            handle.join().expect("allocation worker thread panicked");
        }
    }

    #[test]
    fn test_overflow_protection() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        // 1. Direct call to thread_alloc with size that triggers overflow
        let ptr1 = unsafe {
            mnemosyne_local::thread_alloc::<StandardPolicy, mnemosyne_backend::MemoryBackendWrapper>(
                usize::MAX - 8,
                8,
            )
        };
        assert!(
            ptr1.is_null(),
            "Allocation should fail and return null on overflow"
        );

        // 2. Request a layout of isize::MAX (largest valid layout size) which will fail OS allocation
        let layout = Layout::from_size_align(isize::MAX as usize - 7, 8)
            .expect("isize::MAX - 7 with 8-byte alignment is a valid Layout");
        let ptr2 = unsafe { ALLOCATOR.alloc(layout) };
        assert!(
            ptr2.is_null(),
            "OS allocation should fail and return null for isize::MAX"
        );
    }

    #[test]
    fn test_zero_size_allocation_returns_null() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        let layout =
            Layout::from_size_align(0, 8).expect("zero-size 8-byte aligned Layout is valid");

        let ptr = unsafe { ALLOCATOR.alloc(layout) };
        assert!(ptr.is_null(), "zero-size Mnemosyne alloc returned {ptr:?}");

        let allocator = MnemosyneAllocator::<StandardPolicy>::new();
        let generic_ptr = unsafe { allocator.alloc(layout) };
        assert!(
            generic_ptr.is_null(),
            "zero-size generic allocator returned {generic_ptr:?}"
        );
    }

    #[test]
    fn realloc_within_usable_size_returns_same_pointer_and_preserves_bytes() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        let old_layout =
            Layout::from_size_align(24, 8).expect("24-byte 8-byte aligned Layout is valid");
        let ptr = unsafe { ALLOCATOR.alloc(old_layout) };
        assert!(!ptr.is_null(), "realloc setup allocation failed");
        unsafe {
            core::ptr::write_bytes(ptr, 0xA5, old_layout.size());
        }

        let usable = unsafe { usable_size(ptr) };
        assert!(
            usable >= 32,
            "test requires allocation usable size >= 32, got {usable}"
        );
        let new_ptr = unsafe { ALLOCATOR.realloc(ptr, old_layout, 32) };
        assert_eq!(
            new_ptr, ptr,
            "standard realloc within usable size should stay in place"
        );
        for offset in 0..old_layout.size() {
            let byte = unsafe { *new_ptr.add(offset) };
            assert_eq!(byte, 0xA5, "realloc failed to preserve byte {offset}");
        }

        let new_layout =
            Layout::from_size_align(32, 8).expect("32-byte 8-byte aligned Layout is valid");
        unsafe { ALLOCATOR.dealloc(new_ptr, new_layout) };
    }

    #[test]
    fn secure_realloc_within_usable_size_uses_replacement_allocation() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        let allocator = MnemosyneAllocator::<SecurePolicy>::new();
        let old_layout =
            Layout::from_size_align(24, 8).expect("24-byte 8-byte aligned Layout is valid");
        let ptr = unsafe { allocator.alloc(old_layout) };
        assert!(!ptr.is_null(), "secure realloc setup allocation failed");
        unsafe {
            core::ptr::write_bytes(ptr, 0x5A, old_layout.size());
        }

        let new_ptr = unsafe { allocator.realloc(ptr, old_layout, 32) };
        assert!(
            !new_ptr.is_null(),
            "secure realloc returned null for in-class growth"
        );
        assert_ne!(
            new_ptr, ptr,
            "secure realloc must not grow in place without initializing new bytes"
        );
        for offset in 0..old_layout.size() {
            let byte = unsafe { *new_ptr.add(offset) };
            assert_eq!(
                byte, 0x5A,
                "secure realloc failed to preserve byte {offset}"
            );
        }
        for offset in old_layout.size()..32 {
            let byte = unsafe { *new_ptr.add(offset) };
            assert_eq!(byte, 0, "secure realloc failed to zero new byte {offset}");
        }

        let new_layout =
            Layout::from_size_align(32, 8).expect("32-byte 8-byte aligned Layout is valid");
        unsafe { allocator.dealloc(new_ptr, new_layout) };
    }

    #[test]
    fn realloc_zero_size_returns_null_without_allocating() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        let layout =
            Layout::from_size_align(24, 8).expect("24-byte 8-byte aligned Layout is valid");
        let ptr = unsafe { ALLOCATOR.alloc(layout) };
        assert!(!ptr.is_null(), "zero-size realloc setup allocation failed");
        let new_ptr = unsafe { ALLOCATOR.realloc(ptr, layout, 0) };
        assert!(
            new_ptr.is_null(),
            "zero-size realloc returned non-null pointer {new_ptr:?}"
        );

        let null_realloc = unsafe { ALLOCATOR.realloc(core::ptr::null_mut(), layout, 0) };
        assert!(
            null_realloc.is_null(),
            "null zero-size realloc returned non-null pointer {null_realloc:?}"
        );
    }

    #[test]
    fn test_segment_reclamation() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        // Allocate and deallocate large blocks multiple times
        // If segments are not reclaimed/reused, this would exhaust virtual address space or leak memory.
        for _ in 0..20 {
            let mut allocations = std::vec::Vec::new();
            for _ in 0..10 {
                // Allocate 1MB blocks (large allocations)
                let layout = Layout::from_size_align(1024 * 1024, 8)
                    .expect("1 MiB with 8-byte alignment is a valid Layout");
                let ptr = unsafe { ALLOCATOR.alloc(layout) };
                assert!(
                    !ptr.is_null(),
                    "1 MiB segment-reclamation allocation failed"
                );
                allocations.push((ptr, layout));
            }
            for (ptr, layout) in allocations {
                unsafe { ALLOCATOR.dealloc(ptr, layout) };
            }
        }
    }

    #[test]
    fn test_memory_stats_retention_bound() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        const SIZES: [usize; 40] = [
            8, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96, 104, 112, 120, 128, 160, 192, 224, 256,
            288, 320, 352, 384, 416, 448, 480, 512, 640, 768, 896, 1024, 1152, 1280, 1408, 1536,
            1664, 1792, 1920, 2048,
        ];
        let empty_layout =
            Layout::from_size_align(8, 8).expect("8-byte size and alignment is a valid Layout");
        let mut allocations = [(core::ptr::null_mut(), empty_layout); SIZES.len()];
        let baseline_live_allocations = memory_stats().current_thread_live_allocations;

        for (index, size) in SIZES.into_iter().enumerate() {
            let layout = Layout::from_size_align(size, 8)
                .expect("test size table contains valid 8-byte aligned Layout sizes");
            let ptr = unsafe { ALLOCATOR.alloc(layout) };
            assert!(
                !ptr.is_null(),
                "memory-stats allocation failed for size {size}"
            );
            allocations[index] = (ptr, layout);
        }

        assert!(
            memory_stats().current_thread_live_allocations
                >= baseline_live_allocations + SIZES.len()
        );

        for (ptr, layout) in allocations {
            unsafe { ALLOCATOR.dealloc(ptr, layout) };
        }

        let stats = memory_stats();
        assert!(
            stats.current_mapped_bytes <= stats.peak_mapped_bytes,
            "current_mapped_bytes ({}) exceeds peak_mapped_bytes ({})",
            stats.current_mapped_bytes,
            stats.peak_mapped_bytes
        );
        assert!(
            stats.retained_free_segments <= stats.max_retained_free_segments,
            "retained_free_segments ({}) exceeds bound ({})",
            stats.retained_free_segments,
            stats.max_retained_free_segments
        );
        assert_eq!(
            stats.current_thread_live_allocations,
            baseline_live_allocations
        );
        assert!(stats
            .size_class_occupancy
            .iter()
            .any(|occupancy| occupancy.active_pages > 0));
    }

    #[test]
    fn test_purge() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        // Clear any existing segments in the pool.
        purge();

        let segment = unsafe {
            mnemosyne_arena::allocate_segment::<mnemosyne_backend::MemoryBackendWrapper>()
                .expect("segment allocation must succeed")
        };
        unsafe {
            mnemosyne_arena::deallocate_segment::<mnemosyne_backend::MemoryBackendWrapper>(segment);
        }

        // The segment is now in the global segment pool.
        let stats_before = memory_stats();
        assert!(
            stats_before.retained_free_segments > 0,
            "Expected at least one segment to be retained in the pool"
        );

        purge();

        let stats_after = memory_stats();
        assert_eq!(
            stats_after.retained_free_segments, 0,
            "Expected zero segments to be retained in the pool after purge"
        );
        assert!(
            stats_after.purged_segments > stats_before.purged_segments,
            "Expected purged_segments count to increase"
        );
        assert!(
            stats_after.purge_calls > stats_before.purge_calls,
            "Expected purge_calls count to increase"
        );
        assert!(
            stats_after.purged_bytes > stats_before.purged_bytes,
            "Expected purged_bytes count to increase"
        );
    }

    #[test]
    fn test_reset_keeps_segments_cached_and_records_telemetry() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        // Start from a clean pool so the retention count is deterministic.
        purge();

        // Cache one segment via the standard alloc/dealloc round trip.
        let segment = unsafe {
            mnemosyne_arena::allocate_segment::<mnemosyne_backend::MemoryBackendWrapper>()
                .expect("segment allocation must succeed")
        };
        unsafe {
            mnemosyne_arena::deallocate_segment::<mnemosyne_backend::MemoryBackendWrapper>(segment);
        }
        let stats_before = memory_stats();
        assert!(
            stats_before.retained_free_segments >= 1,
            "expected at least one cached segment before reset"
        );

        reset();

        let stats_after = memory_stats();
        // Reset preserves the cache: retention count does not drop. The
        // process-wide retained pool may grow if another completed test
        // thread's TLS allocator returns an owned segment concurrently.
        assert!(
            stats_after.retained_free_segments >= stats_before.retained_free_segments,
            "reset must not evict retained segments: before={} after={}",
            stats_before.retained_free_segments,
            stats_after.retained_free_segments
        );
        // Reset always increments its own call counter, regardless of
        // whether the backend confirmed the page-reset advice.
        assert!(
            stats_after.reset_calls > stats_before.reset_calls,
            "reset_calls counter did not advance: before={} after={}",
            stats_before.reset_calls,
            stats_after.reset_calls
        );
        // On Windows the wrapper backend implements page_reset via
        // VirtualAlloc(MEM_RESET) which always succeeds for active
        // mappings, so reset_segments should also advance. On platforms
        // where the kernel declines the advice, this is permitted to
        // stay equal — the test asserts only the call counter advanced.
        assert!(
            stats_after.reset_segments >= stats_before.reset_segments,
            "reset_segments regressed: before={} after={}",
            stats_before.reset_segments,
            stats_after.reset_segments
        );
        // Purge counters are not perturbed by reset.
        assert_eq!(
            stats_after.purge_calls, stats_before.purge_calls,
            "reset must not increment purge_calls"
        );
        assert_eq!(
            stats_after.purged_segments, stats_before.purged_segments,
            "reset must not increment purged_segments"
        );

        // The address space remains writable through the cached mapping
        // — drain the pool to pop the segment and write through it.
        purge();
    }

    #[test]
    fn test_realloc_within_class_returns_same_ptr() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        // 32 B request lands in size class 1 (block_size = 32 B); shrinking
        // and growing-within-class must both return the same pointer with
        // no copy-and-free.
        let layout = Layout::from_size_align(32, 8).expect("valid layout");
        let ptr = unsafe { ALLOCATOR.alloc(layout) };
        assert!(!ptr.is_null());

        // Mark a sentinel byte so we can detect any unintended copy.
        unsafe { ptr.write(0x5A) };

        // Shrink within class.
        let shrunk = unsafe { ALLOCATOR.realloc(ptr, layout, 16) };
        assert_eq!(
            shrunk, ptr,
            "shrink within class must return the same pointer"
        );

        // Grow within class.
        let grown = unsafe { ALLOCATOR.realloc(shrunk, layout, 32) };
        assert_eq!(grown, ptr, "grow within class must return the same pointer");

        // Confirm the sentinel survived — no copy happened.
        assert_eq!(
            unsafe { ptr.read() },
            0x5A,
            "sentinel byte mutated; an unwanted copy occurred"
        );

        unsafe { ALLOCATOR.dealloc(ptr, layout) };
    }

    #[test]
    fn test_realloc_across_class_copies_and_returns_new_ptr() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        // 16 B request → class 0 (block_size 16). Growing to 64 B requires
        // a different size class; the realloc must allocate, copy, and
        // free. The original sentinel bytes must appear in the new
        // allocation.
        let small_layout = Layout::from_size_align(16, 8).expect("valid layout");
        let ptr = unsafe { ALLOCATOR.alloc(small_layout) };
        assert!(!ptr.is_null());
        // Fill the 16 B with a known pattern.
        for i in 0..16usize {
            unsafe { ptr.add(i).write((i as u8).wrapping_add(0xA0)) };
        }

        let new_ptr = unsafe { ALLOCATOR.realloc(ptr, small_layout, 64) };
        assert!(!new_ptr.is_null());

        // The new allocation may or may not coincide with `ptr` depending
        // on the size-class choice; what matters is that the prefix
        // bytes were preserved.
        for i in 0..16usize {
            assert_eq!(
                unsafe { new_ptr.add(i).read() },
                (i as u8).wrapping_add(0xA0),
                "realloc across class did not preserve byte {i}"
            );
        }

        let new_layout = Layout::from_size_align(64, 8).expect("valid layout");
        unsafe { ALLOCATOR.dealloc(new_ptr, new_layout) };
    }

    #[test]
    fn test_realloc_does_not_copy_past_layout_size() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        // Pins the slow-path copy-length contract: even when the caller's
        // allocation has size-class slack (usable_size > layout.size), the
        // slow path must copy *only* layout.size bytes. If it instead
        // copied usable_size bytes, an accidental write in the slack
        // region would propagate to the new allocation.
        //
        // Setup: 8 B request lands in class 0 (block_size 16 B), so
        // layout.size = 8 but usable_size(ptr) = 16. Use SecurePolicy so
        // the replacement allocation has defined zero bytes beyond the
        // copied user region; this lets the test inspect [8, 16) without
        // reading uninitialized memory.
        let allocator = MnemosyneAllocator::<SecurePolicy>::new();
        let small_layout = Layout::from_size_align(8, 8).expect("valid layout");
        let ptr = unsafe { allocator.alloc(small_layout) };
        assert!(!ptr.is_null());
        // Sanity-check the slack window exists.
        let reported = unsafe { mnemosyne_local::usable_size(ptr) };
        assert!(
            reported >= 16,
            "8 B request must land in a class with at least 16 B usable; got {reported}"
        );

        // User region: bytes 0..8.
        for i in 0..8usize {
            unsafe { ptr.add(i).write(0xAA) };
        }
        // Slack region: bytes 8..16. Mnemosyne lets you safely write up to
        // usable_size bytes, so this is well-defined; but the realloc copy
        // must not pull this into the new allocation.
        for i in 8..16usize {
            unsafe { ptr.add(i).write(0xBB) };
        }

        // Cross-class grow.
        let new_ptr = unsafe { allocator.realloc(ptr, small_layout, 64) };
        assert!(!new_ptr.is_null());

        for i in 0..8usize {
            assert_eq!(
                unsafe { new_ptr.add(i).read() },
                0xAA,
                "realloc must preserve the {i}-th user byte"
            );
        }
        for i in 8..16usize {
            assert_eq!(
                unsafe { new_ptr.add(i).read() },
                0,
                "secure realloc copied slack byte {i} past layout.size"
            );
        }

        let new_layout = Layout::from_size_align(64, 8).expect("valid layout");
        unsafe { allocator.dealloc(new_ptr, new_layout) };
    }

    #[test]
    fn test_realloc_null_ptr_acts_as_alloc() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        let layout = Layout::from_size_align(0, 8).expect("valid layout");
        let ptr = unsafe { ALLOCATOR.realloc(core::ptr::null_mut(), layout, 128) };
        assert!(!ptr.is_null(), "realloc(null, 128) must allocate");
        let new_layout = Layout::from_size_align(128, 8).expect("valid layout");
        unsafe { ALLOCATOR.dealloc(ptr, new_layout) };
    }

    #[test]
    fn test_realloc_to_zero_size_frees() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        let layout = Layout::from_size_align(32, 8).expect("valid layout");
        let ptr = unsafe { ALLOCATOR.alloc(layout) };
        assert!(!ptr.is_null());

        let null = unsafe { ALLOCATOR.realloc(ptr, layout, 0) };
        assert!(null.is_null(), "realloc(_, 0) must return null after free");
    }

    #[test]
    fn test_large_alignment() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        let alignments = [32 * 1024, 64 * 1024, 128 * 1024, 2 * 1024 * 1024];
        for align in alignments {
            let layout = Layout::from_size_align(4096, align)
                .expect("large-alignment test table contains valid Layout alignments");
            let ptr = unsafe { ALLOCATOR.alloc(layout) };
            assert!(!ptr.is_null(), "Allocation failed for alignment {}", align);
            assert_eq!(
                ptr as usize % align,
                0,
                "Pointer {:?} is not aligned to {}",
                ptr,
                align
            );
            // Verify writing and reading to make sure alignment bounds check out.
            unsafe {
                ptr.write(0xAA);
                assert_eq!(ptr.read(), 0xAA);
                ptr.add(4095).write(0x55);
                assert_eq!(ptr.add(4095).read(), 0x55);
            }
            unsafe { ALLOCATOR.dealloc(ptr, layout) };
        }
    }

    #[test]
    fn test_secure_policy() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        let allocator = MnemosyneAllocator::<SecurePolicy>::new();
        let layout = Layout::from_size_align(128, 8).expect("128-byte 8-aligned Layout is valid");

        // 1. Test zero-initialization
        let ptr = unsafe { allocator.alloc(layout) };
        assert!(!ptr.is_null(), "secure-policy allocation failed");

        // Verify that the memory is indeed zero-initialized
        let slice = unsafe { core::slice::from_raw_parts(ptr, 128) };
        for &byte in slice {
            assert_eq!(byte, 0, "Byte was not zero-initialized");
        }

        // 2. Test memory poisoning on deallocation.
        // We write some sentinel values before freeing to ensure it's overwritten by poison bytes.
        unsafe {
            core::ptr::write_bytes(ptr, 0x41, 128);
        }

        unsafe { allocator.dealloc(ptr, layout) };

        // Safety: Under standard execution, accessing freed memory is undefined behavior.
        // However, in this controlled integration test, we verify that the poisoning logic
        // has overwritten the memory. The segment cache retains pages so the memory
        // remains mapped and readable for testing.
        let skip_bytes =
            core::mem::size_of::<Option<core::ptr::NonNull<mnemosyne_core::types::Block>>>();
        for i in skip_bytes..128 {
            let val = unsafe { ptr.add(i).read() };
            assert_eq!(
                val, 0xDE,
                "Byte at index {} was not poisoned (got 0x{:02X}, expected 0xDE)",
                i, val
            );
        }
    }

    #[test]
    fn test_cuda_unified_backend() {
        let _guard = TEST_LOCK
            .lock()
            .expect("global allocator test lock was poisoned");
        let allocator = MnemosyneAllocator::<StandardPolicy, CudaUnifiedBackend>::new();
        let layout = Layout::from_size_align(128, 8).expect("128-byte 8-aligned Layout is valid");
        let ptr = unsafe { allocator.alloc(layout) };
        assert!(!ptr.is_null(), "CUDA unified backend allocation failed");

        unsafe {
            ptr.write(42);
            assert_eq!(ptr.read(), 42);
            allocator.dealloc(ptr, layout);
        }

        // Verify statistics generic query works for CUDA backend
        let stats = memory_stats_generic::<CudaUnifiedBackend>();
        assert_eq!(stats.current_thread_live_allocations, 0);

        // Verify is_cuda_available is callable
        let _ = is_cuda_available();
    }
}
