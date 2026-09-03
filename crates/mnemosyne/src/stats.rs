use mnemosyne_core::NUM_SIZE_CLASSES;

use crate::{LocalAllocatorSelector, SizeClassOccupancy};

/// Snapshot of Mnemosyne memory mapping and segment cache state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MemoryStats {
    /// Address space currently mapped from the OS, in bytes. Reserved
    /// space, not resident: a range stays counted after its physical
    /// backing is released, because the mapping is still held.
    pub current_mapped_bytes: usize,
    /// High-water mark of [`Self::current_mapped_bytes`].
    pub peak_mapped_bytes: usize,
    /// Successful map requests to the OS.
    pub map_calls: usize,
    /// Successful unmap requests to the OS.
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
    /// Free segments held in the cache for reuse instead of unmapped.
    pub retained_free_segments: usize,
    /// Cap on [`Self::retained_free_segments`]; segments beyond it are
    /// purged rather than retained.
    pub max_retained_free_segments: usize,
    /// Bytes represented by the retained free segments.
    pub retained_free_bytes: usize,
    /// Segments returned to the OS by decay.
    pub purged_segments: usize,
    /// Decay purge passes performed.
    pub purge_calls: usize,
    /// Bytes returned to the OS by those purges.
    pub purged_bytes: usize,
    /// Number of segments whose physical backing was released by a
    /// confirmed `page_reset` while the segment itself remained cached
    /// in the retained pool.
    pub reset_segments: usize,
    /// Number of `reset_segment_pool` invocations.
    pub reset_calls: usize,
    /// Number of huge blocks currently retained in the huge-allocation cache
    /// across all NUMA nodes.
    pub retained_huge_blocks: usize,
    /// Total bytes of huge blocks currently retained in the huge-allocation
    /// cache across all NUMA nodes.
    pub retained_huge_bytes: usize,
    /// Allocations currently handed out by the calling thread.
    pub current_thread_live_allocations: usize,
    /// Segments the calling thread owns and allocates from without
    /// coordination.
    pub current_thread_owned_segments: usize,
    /// Blocks freed by another thread and drained back into this
    /// thread's pages.
    pub cross_thread_reclaimed_blocks: usize,
    /// Times a size class exhausted its page and acquired another; the
    /// sum of the three sources below.
    pub page_refills: usize,
    /// Refills served from an already-held empty page, the cheapest
    /// outcome.
    pub recycled_pages: usize,
    /// Refills that carved a new page from an owned segment.
    pub fresh_pages: usize,
    /// Refills that needed a new segment, the only source reaching the
    /// OS backend.
    pub fresh_segments: usize,
    /// Segments inherited from threads that exited still owning them,
    /// which keeps their memory reusable rather than stranded.
    pub orphan_segments_adopted: usize,
    /// Decay-sweep passes over pages looking for empties to recycle.
    ///
    /// Against [`Self::recycled_pages`] this shows whether sweeping is
    /// paying for itself or scanning without finding reusable pages.
    pub recycle_sweeps: usize,
    /// Per-size-class occupancy for the calling thread, indexed by size
    /// class.
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
            retained_huge_blocks: 0,
            retained_huge_bytes: 0,
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

/// Returns current Mnemosyne allocator memory counters for a specific policy
/// and backend.
///
/// `P` must be the policy the caller allocates with — an application using
/// `MnemosyneAllocator<HardenedPolicy>` passes `HardenedPolicy` here. Each
/// `(backend, encryption mode)` pair owns a separate thread allocator cache
/// (ADR 0001), so naming the wrong policy reports a different allocator's
/// counters rather than failing (ADR 0008).
pub fn memory_stats_generic<
    P: crate::AllocPolicy,
    B: mnemosyne_arena::HasSegmentPool + LocalAllocatorSelector<B>,
>() -> MemoryStats {
    let backend = mnemosyne_backend::backend_memory_stats();
    let arena = mnemosyne_arena::arena_memory_stats::<B>();
    let local = mnemosyne_local::thread_allocator_stats::<P, B>();
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
        retained_huge_blocks: arena.retained_huge_blocks,
        retained_huge_bytes: arena.retained_huge_bytes,
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

/// Returns current Mnemosyne allocator memory counters for the default
/// `Mnemosyne` allocator.
///
/// `Mnemosyne` is `MnemosyneAllocator<StandardPolicy>`'s shorthand, and this is
/// `memory_stats_generic::<StandardPolicy, _>`'s. A process installing any
/// other policy calls the generic form with that policy, or it reads a
/// different allocator's counters.
pub fn memory_stats() -> MemoryStats {
    memory_stats_generic::<crate::StandardPolicy, mnemosyne_backend::MemoryBackendWrapper>()
}

/// Purges the global segment pool for a specific backend, releasing all retained/cached segments back to the OS.
pub fn purge_generic<B: mnemosyne_arena::HasSegmentPool>() {
    // SAFETY: Purging the segment pool releases only free segments that are
    // no longer actively referenced by any thread allocator cache.
    unsafe {
        mnemosyne_arena::purge_segment_pool::<B>();
    }
}

/// Purges the global segment pool, releasing all retained/cached segments back to the OS.
pub fn purge() {
    purge_generic::<mnemosyne_backend::MemoryBackendWrapper>();
}

/// Purges the global segment pool while keeping `warm_threshold` committed
/// segments ready for immediate reuse.
///
/// Unlike [`purge`], which releases everything, this keeps the `warm_threshold`
/// most-recently-retained segments in the pool so that a burst of allocations
/// immediately following the purge avoids `VirtualAlloc`/`mmap` round-trips.
///
/// Callers with a policy-level guidance should pass
/// `P::SEGMENT_POOL_WARM_THRESHOLD`. Pass `0` for the same behavior as `purge`.
pub fn purge_lazy(warm_threshold: usize) {
    // SAFETY: purge_segment_pool_with_warm only touches segments the pool owns
    // exclusively; the global pool itself serialises access to its free list.
    unsafe {
        mnemosyne_arena::purge_segment_pool_with_warm::<mnemosyne_backend::MemoryBackendWrapper>(
            warm_threshold,
        );
    }
}

/// Asks the OS to drop the physical backing of every retained free
/// segment for a specific backend without removing them from the cache.
///
/// Use this as a lighter-weight RSS-reduction knob than `purge`: the
/// segment cache stays warm so subsequent allocations skip the OS
/// mapping syscall, while the resident memory footprint of idle
/// segments drops to the kernel's demand-fault baseline.
pub fn reset_generic<B: mnemosyne_arena::HasSegmentPool>() {
    // SAFETY: reset_segment_pool drains the retained pool, issues
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

/// Returns a JSON string containing a full Mnemosyne memory stats snapshot
/// plus per-size-class bin counters and aggregate fragmentation metrics.
///
/// The returned JSON object contains:
/// - All fields from [`MemoryStats`] as top-level numeric keys.
/// - `bins`: array of per-class objects with `block_size`, `alloc_count`,
///   `dealloc_count`, `live_estimate`, and `fragmentation_ratio`.
/// - `bin_totals`: aggregate object with `total_alloc_count`, `total_live_bytes`,
///   and `hottest_class` (-1 when nothing has been allocated).
///
/// Intended for diagnostic logging and health-check endpoints. The format
/// may evolve between versions; callers should not depend on field order.
///
/// Calls [`flush_tls_stats`][mnemosyne_local::flush_tls_stats] before
/// sampling so that allocations made on the calling thread but not yet flushed
/// to the global counters appear in the snapshot.
pub fn memory_stats_json() -> alloc::string::String {
    use alloc::format;
    use alloc::string::String;

    // Flush the calling thread's accumulated bin stats into the global
    // atomics so the snapshot reflects all activity on this thread.
    mnemosyne_local::flush_tls_stats();

    let s = memory_stats();
    let bins = mnemosyne_local::all_bin_snapshots();
    let total_allocs = mnemosyne_local::total_alloc_count();
    let live_bytes = mnemosyne_local::total_live_bytes();
    let hottest = mnemosyne_local::hottest_class()
        .map(|c| c as i64)
        .unwrap_or(-1);

    let mut out = String::with_capacity(8192);
    out.push('{');

    macro_rules! kv {
        ($key:expr, $val:expr, $comma:expr) => {
            if $comma { out.push(','); }
            out.push('"');
            out.push_str($key);
            out.push_str("\":");
            out.push_str(&format!("{}", $val));
        };
    }

    kv!("current_mapped_bytes",     s.current_mapped_bytes, false);
    kv!("peak_mapped_bytes",         s.peak_mapped_bytes, true);
    kv!("map_calls",                 s.map_calls, true);
    kv!("unmap_calls",               s.unmap_calls, true);
    kv!("page_reset_calls",          s.page_reset_calls, true);
    kv!("page_reset_bytes",          s.page_reset_bytes, true);
    kv!("retained_free_segments",    s.retained_free_segments, true);
    kv!("max_retained_free_segments",s.max_retained_free_segments, true);
    kv!("retained_free_bytes",       s.retained_free_bytes, true);
    kv!("purged_segments",           s.purged_segments, true);
    kv!("purge_calls",               s.purge_calls, true);
    kv!("purged_bytes",              s.purged_bytes, true);
    kv!("reset_segments",            s.reset_segments, true);
    kv!("reset_calls",               s.reset_calls, true);
    kv!("retained_huge_blocks",      s.retained_huge_blocks, true);
    kv!("retained_huge_bytes",       s.retained_huge_bytes, true);
    kv!("current_thread_live_allocations", s.current_thread_live_allocations, true);
    kv!("current_thread_owned_segments",   s.current_thread_owned_segments, true);
    kv!("cross_thread_reclaimed_blocks",   s.cross_thread_reclaimed_blocks, true);
    kv!("page_refills",              s.page_refills, true);
    kv!("recycled_pages",            s.recycled_pages, true);
    kv!("fresh_pages",               s.fresh_pages, true);
    kv!("fresh_segments",            s.fresh_segments, true);

    // Per-bin array with fragmentation ratio.
    out.push_str(",\"bins\":[");
    for (i, bin) in bins.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push_str(&format!(
            "{{\"block_size\":{},\"alloc_count\":{},\"dealloc_count\":{},\
             \"live_estimate\":{},\"fragmentation_ratio\":{:.4}}}",
            bin.block_size, bin.alloc_count, bin.dealloc_count,
            bin.live_estimate, bin.fragmentation_ratio()
        ));
    }

    // Aggregate totals.
    out.push_str(&format!(
        "],\"bin_totals\":{{\"total_alloc_count\":{},\"total_live_bytes\":{},\
         \"hottest_class\":{}}}}}",
        total_allocs, live_bytes, hottest
    ));
    out
}
