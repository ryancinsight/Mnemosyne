//! The snapshot type and its derived views.

use core::fmt::Write as _;

use mnemosyne_core::NUM_SIZE_CLASSES;

use crate::SizeClassOccupancy;

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

impl MemoryStats {
    /// Serializes this snapshot plus per-bin counters to a JSON string.
    ///
    /// Use [`crate::stats::memory_stats_json`] for a convenient one-call version that
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
