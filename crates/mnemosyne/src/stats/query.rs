//! Reading a snapshot from the live allocator.

use super::types::MemoryStats;
use crate::LocalAllocatorSelector;

/// Returns current Mnemosyne allocator memory counters for a specific policy
/// and backend.
///
/// `P` must be the policy the caller allocates with — an application using
/// `MnemosyneAllocator<HardenedPolicy>` passes `HardenedPolicy` here. Each
/// `(backend, encryption mode)` pair owns a separate thread allocator cache
/// (ADR 0001), so naming the wrong policy reports a different allocator's
/// counters rather than failing (ADR 0008).
pub fn memory_stats_generic<
    P: crate::AllocPolicy + mnemosyne_local::tls_slot::PolicySlotSelection<B>,
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

/// Returns a JSON representation of the current memory statistics and
/// per-size-bin counters as an owned `String`.
///
/// The output is a single JSON object suitable for structured logging,
/// dashboards, or diagnostic endpoints.  No external dependencies are
/// required: the JSON is hand-rolled so this function is available in
/// `no_std + alloc` contexts.
///
/// # Output shape
///
/// ```json
/// {
///   "current_mapped_bytes": 2097152,
///   "peak_mapped_bytes": 4194304,
///   ...
///   "bins": [
///     {"block_size": 16, "alloc_count": 1024, "dealloc_count": 1023, "live_estimate": 1},
///     ...
///   ]
/// }
/// ```
pub fn memory_stats_json() -> alloc::string::String {
    let s = memory_stats();
    let bins = mnemosyne_local::all_bin_snapshots();
    let mut json = s.to_json_with_bins(&bins);
    // Insert compile-time policy metadata before the closing brace.
    // The policy name and mitigation flags let parsers correlate a dump
    // with the build configuration without consulting the binary.
    use mnemosyne_core::policy::AllocPolicy;
    json.pop(); // remove trailing '}'
    // Include segment and huge pool telemetry.
    use mnemosyne_arena::HasSegmentPool as _;
    let sp = mnemosyne_backend::MemoryBackendWrapper::global_segment_pool().stats();
    let hp = mnemosyne_backend::MemoryBackendWrapper::global_huge_pool().stats();
    let total_req = mnemosyne_local::total_requested_bytes();
    let int_frag = mnemosyne_local::total_internal_fragmentation();
    let reset_gen = mnemosyne_local::reset_generation_count();
    let _ = ::core::fmt::Write::write_fmt(
        &mut json,
        core::format_args!(
            ",\"pool_retained\":{},\"pool_purged\":{},\"pool_purge_calls\":{},\
             \"pool_reset_segments\":{},\"pool_reset_calls\":{},\
             \"huge_retained_blocks\":{},\"huge_retained_bytes\":{},\
             \"total_requested_bytes\":{},\"total_internal_fragmentation\":{:.4},\
             \"reset_generation\":{},\
             \"policy_name\":\"{}\",\"mitigation_flags\":{},\"policy_fingerprint\":{}}}",
            sp.retained,
            sp.purged_segments,
            sp.purge_calls,
            sp.reset_segments,
            sp.reset_calls,
            hp.retained_blocks,
            hp.retained_bytes,
            total_req,
            int_frag,
            reset_gen,
            mnemosyne_core::policy::StandardPolicy::POLICY_NAME,
            mnemosyne_core::policy::StandardPolicy::MITIGATION_FLAGS,
            mnemosyne_core::policy::StandardPolicy::POLICY_FINGERPRINT,
        ),
    );
    json
}
