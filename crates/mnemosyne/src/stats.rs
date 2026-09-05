use core::fmt::Write as _;

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

impl MemoryStats {
    /// Serializes this snapshot plus per-bin counters to a JSON string.
    ///
    /// Use [`memory_stats_json`] for a convenient one-call version that
    /// captures the current stats automatically.
    pub fn to_json_with_bins(
        &self,
        bins: &[mnemosyne_local::BinSnapshot],
    ) -> alloc::string::String {
        use alloc::format;
        use alloc::string::String;

        let mut out = String::with_capacity(4096);
        out.push('{');
        macro_rules! kv_usize {
            ($key:expr, $val:expr, $comma:expr) => {
                if $comma {
                    out.push(',');
                }
                out.push('"');
                out.push_str($key);
                out.push_str("\":");
                out.push_str(&format!("{}", $val));
            };
        }
        kv_usize!("current_mapped_bytes", self.current_mapped_bytes, false);
        kv_usize!("peak_mapped_bytes", self.peak_mapped_bytes, true);
        kv_usize!("map_calls", self.map_calls, true);
        kv_usize!("unmap_calls", self.unmap_calls, true);
        kv_usize!("page_reset_calls", self.page_reset_calls, true);
        kv_usize!("page_reset_bytes", self.page_reset_bytes, true);
        kv_usize!(
            "decommit_bytes",
            mnemosyne_backend::backend_memory_stats().decommit_bytes,
            true
        );
        kv_usize!("purged_bytes", self.purged_bytes, true);
        kv_usize!("retained_free_segments", self.retained_free_segments, true);
        kv_usize!(
            "max_retained_free_segments",
            self.max_retained_free_segments,
            true
        );
        kv_usize!("retained_free_bytes", self.retained_free_bytes, true);
        kv_usize!("purged_segments", self.purged_segments, true);
        kv_usize!("purge_calls", self.purge_calls, true);
        kv_usize!("reset_segments", self.reset_segments, true);
        kv_usize!("reset_calls", self.reset_calls, true);
        kv_usize!("retained_huge_blocks", self.retained_huge_blocks, true);
        kv_usize!("retained_huge_bytes", self.retained_huge_bytes, true);
        kv_usize!(
            "current_thread_live_allocations",
            self.current_thread_live_allocations,
            true
        );
        kv_usize!(
            "current_thread_owned_segments",
            self.current_thread_owned_segments,
            true
        );
        kv_usize!(
            "cross_thread_reclaimed_blocks",
            self.cross_thread_reclaimed_blocks,
            true
        );
        kv_usize!("page_refills", self.page_refills, true);
        kv_usize!("recycled_pages", self.recycled_pages, true);
        kv_usize!("fresh_pages", self.fresh_pages, true);
        kv_usize!("fresh_segments", self.fresh_segments, true);
        kv_usize!(
            "orphan_segments_adopted",
            self.orphan_segments_adopted,
            true
        );
        kv_usize!("recycle_sweeps", self.recycle_sweeps, true);
        // Per-bin array
        out.push_str(",\"bins\":[");
        for (i, bin) in bins.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            // `write!` formats straight into `out`; `push_str(&format!(..))`
            // would allocate a second `String` per bin only to copy it in.
            let _ = write!(
                out,
                "{{\"block_size\":{},\"alloc_count\":{},\"dealloc_count\":{},\
                 \"live_estimate\":{},\"requested_bytes\":{},\
                 \"fragmentation\":{:.4},\"internal_fragmentation\":{:.4}}}",
                bin.block_size,
                bin.alloc_count,
                bin.dealloc_count,
                bin.live_estimate,
                bin.requested_bytes,
                bin.fragmentation_ratio(),
                bin.internal_fragmentation_ratio()
            );
        }
        out.push_str("]}");
        out
    }
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

/// Asks the OS to drop the physical backing of every retained standard free
/// segment for a specific backend without removing them from the cache.
///
/// Use this as a lighter-weight RSS-reduction knob than `purge`: the
/// standard segment cache stays warm so subsequent small allocations skip the
/// OS mapping syscall, while the resident memory footprint of idle segments
/// drops to the kernel's demand-fault baseline. The separate huge-allocation
/// cache is not reset by this operation.
pub fn reset_generic<B: mnemosyne_arena::HasSegmentPool>() {
    // SAFETY: reset_segment_pool drains the retained pool, issues
    // page_reset on each segment's mapping, and pushes them back into
    // the cache; no segment is released or accessed by another path.
    unsafe {
        mnemosyne_arena::reset_segment_pool::<B>();
    }
}

/// Asks the OS to drop the physical backing of every retained standard free
/// segment without removing them from the cache.
///
/// The separate huge-allocation cache is not reset; use [`purge`] when both
/// cache families must be released.
pub fn reset() {
    reset_generic::<mnemosyne_backend::MemoryBackendWrapper>();
}

/// Purges the global segment pool while keeping `warm_threshold` committed
/// segments ready for immediate reuse.
///
/// Unlike [`purge`], which releases everything, this keeps the
/// `warm_threshold` most-recently-retained segments committed so that a
/// burst of allocations immediately following the purge avoids
/// `VirtualAlloc`/`mmap` round-trips.
///
/// Pass `P::SEGMENT_POOL_WARM_THRESHOLD` for policy-level guidance, or `0`
/// for the same behaviour as `purge`.
pub fn purge_lazy(warm_threshold: usize) {
    // SAFETY: purge_segment_pool_with_warm only touches segments the pool
    // owns exclusively.
    unsafe {
        mnemosyne_arena::purge_segment_pool_with_warm::<mnemosyne_backend::MemoryBackendWrapper>(
            warm_threshold,
        );
    }
}

/// Purges with `StandardPolicy::SEGMENT_POOL_WARM_THRESHOLD` warm segments kept.
///
/// A higher-level shortcut for `purge_lazy(StandardPolicy::SEGMENT_POOL_WARM_THRESHOLD)`.
pub fn purge_standard() {
    use mnemosyne_core::policy::AllocPolicy;
    purge_lazy(mnemosyne_core::policy::StandardPolicy::SEGMENT_POOL_WARM_THRESHOLD);
}

/// Triggers a manual background decay and defragmentation cycle across all active memory backends.
pub fn decay() {
    mnemosyne_decay::decay_step();
}

/// Returns a human-readable one-line summary of the current policy and telemetry.
///
/// Format: `policy=<name> mitigations=0x<flags> allocs=<n> live_bytes=<b> int_frag=<x>%`
#[must_use]
pub fn policy_summary() -> alloc::string::String {
    use alloc::format;
    use mnemosyne_core::policy::AllocPolicy;
    let stats = mnemosyne_local::summary_line();
    format!(
        "policy={} mitigations=0x{:08X} fingerprint=0x{:016X} {stats}",
        mnemosyne_core::policy::StandardPolicy::POLICY_NAME,
        mnemosyne_core::policy::StandardPolicy::MITIGATION_FLAGS,
        mnemosyne_core::policy::StandardPolicy::POLICY_FINGERPRINT,
    )
}

/// Returns the `n` hottest size classes by alloc_count, sorted descending.
///
/// Flushes TLS stats before sampling. Returns at most `n` entries; fewer
/// if fewer than `n` classes have been allocated from.
#[must_use]
pub fn top_n_classes(n: usize) -> alloc::vec::Vec<mnemosyne_local::BinSnapshot> {
    let mut snapshots: alloc::vec::Vec<_> = mnemosyne_local::all_bin_snapshots()
        .into_iter()
        .filter(|s| s.alloc_count > 0)
        .collect();
    snapshots.sort_unstable_by(|a, b| b.alloc_count.cmp(&a.alloc_count));
    snapshots.truncate(n);
    snapshots
}

// ── Stats window ──────────────────────────────────────────────────────────────

/// A snapshot of bin stats taken at a fixed point in time.
///
/// Create a baseline with [`BinStatsWindow::capture`], then call
/// [`BinStatsWindow::delta`] later to compute per-class deltas over the window.
/// This is the recommended pattern for profiling a code region:
///
/// ```rust
/// # use mnemosyne::BinStatsWindow;
/// let baseline = BinStatsWindow::capture();
/// // ... code under profiling ...
/// let delta = baseline.delta();
/// ```
pub struct BinStatsWindow {
    bins: [mnemosyne_local::BinSnapshot; mnemosyne_core::NUM_SIZE_CLASSES],
}

impl BinStatsWindow {
    /// Captures the current per-class bin stats as a baseline.
    ///
    /// Flushes the calling thread's TLS batch first so the snapshot
    /// reflects all preceding allocations on this thread.
    #[must_use]
    pub fn capture() -> Self {
        Self {
            bins: mnemosyne_local::all_bin_snapshots(),
        }
    }

    /// Computes per-class deltas since the baseline was captured.
    ///
    /// Each returned snapshot has its counters set to the difference since
    /// the baseline. Counters that decreased (or were reset) saturate to zero.
    #[must_use]
    pub fn delta(&self) -> [mnemosyne_local::BinSnapshot; mnemosyne_core::NUM_SIZE_CLASSES] {
        let now = mnemosyne_local::all_bin_snapshots();
        core::array::from_fn(|class| {
            let b = &self.bins[class];
            let n = &now[class];
            n.saturating_delta(b)
        })
    }

    /// Total allocations during the window across all size classes.
    #[must_use]
    pub fn total_alloc_count_delta(&self) -> u64 {
        self.delta()
            .iter()
            .map(|s| s.alloc_count)
            .fold(0u64, u64::saturating_add)
    }

    /// Total live bytes at the end of the window minus the start.
    #[must_use]
    pub fn total_live_bytes_delta(&self) -> u64 {
        self.delta()
            .iter()
            .map(|s| s.live_bytes())
            .fold(0u64, u64::saturating_add)
    }

    /// Total user-requested bytes during the window across all size classes.
    ///
    /// Requires `record_alloc_with_size` to have been used at call sites.
    #[must_use]
    pub fn total_requested_bytes_delta(&self) -> u64 {
        self.delta()
            .iter()
            .map(|s| s.requested_bytes)
            .fold(0u64, u64::saturating_add)
    }

    /// Internal fragmentation ratio over the window:
    /// `(total_alloc_bytes_delta - total_requested_bytes_delta) / total_alloc_bytes_delta`.
    ///
    /// Returns `0.0` when `total_alloc_bytes_delta == 0` or `requested` is zero.
    #[must_use]
    pub fn window_internal_fragmentation(&self) -> f64 {
        let d = self.delta();
        let alloc: u64 = d.iter().map(|s| s.alloc_bytes).fold(0, u64::saturating_add);
        let req: u64 = d.iter().map(|s| s.requested_bytes).fold(0, u64::saturating_add);
        if alloc == 0 || req == 0 {
            return 0.0;
        }
        let waste = alloc.saturating_sub(req);
        (waste as f64 / alloc as f64).min(1.0)
    }
}
