use core::alloc::{GlobalAlloc, Layout};

// Safety: all report layouts use fixed positive sizes and nonzero
// power-of-two alignments.
const LAYOUTS: [Layout; 3] = [
    unsafe { Layout::from_size_align_unchecked(32, 8) },
    unsafe { Layout::from_size_align_unchecked(1024, 8) },
    unsafe { Layout::from_size_align_unchecked(8192, 8) },
];
const ALLOCS_PER_LAYOUT: usize = 128;
const SEGMENT_EVICTION_ALLOCS: usize = mnemosyne_arena::MAX_RETAINED_SEGMENTS + 8;

fn main() -> Result<(), &'static str> {
    let before = mnemosyne::memory_stats();
    let mut allocations = Vec::with_capacity(LAYOUTS.len() * ALLOCS_PER_LAYOUT);

    for layout in LAYOUTS {
        for _ in 0..ALLOCS_PER_LAYOUT {
            // Safety: `layout` comes from the static valid report layout table.
            let ptr = unsafe { mnemosyne::Mnemosyne.alloc(layout) };
            if ptr.is_null() {
                return Err("Allocation returned a null pointer");
            }
            allocations.push((ptr, layout));
        }
    }

    let during = mnemosyne::memory_stats();

    for (ptr, layout) in allocations {
        // Safety: each `(ptr, layout)` pair was allocated above by Mnemosyne
        // and is deallocated exactly once in this loop.
        unsafe {
            mnemosyne::Mnemosyne.dealloc(ptr, layout);
        }
    }

    let after = mnemosyne::memory_stats();
    println!("phase,current_mapped_bytes,peak_mapped_bytes,map_calls,unmap_calls,retained_free_segments,max_retained_free_segments,retained_free_bytes,purged_segments,purge_calls,purged_bytes,current_thread_live_allocations,current_thread_owned_segments,cross_thread_reclaimed_blocks,page_refills,recycled_pages,fresh_pages,fresh_segments,orphan_segments_adopted,recycle_sweeps");
    print_stats("before", before);
    print_stats("during", during);
    print_stats("after", after);
    let eviction_after = run_segment_eviction()?;
    print_stats("eviction_after", eviction_after);
    let purge_after = purge_segment_cache()?;
    print_stats("purge_after", purge_after);
    println!("phase,size_class,active_pages,empty_pages,live_allocations,total_slots");
    print_occupancy("after", after);
    Ok(())
}

fn print_stats(phase: &str, stats: mnemosyne::MemoryStats) {
    println!(
        "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        phase,
        stats.current_mapped_bytes,
        stats.peak_mapped_bytes,
        stats.map_calls,
        stats.unmap_calls,
        stats.retained_free_segments,
        stats.max_retained_free_segments,
        stats.retained_free_bytes,
        stats.purged_segments,
        stats.purge_calls,
        stats.purged_bytes,
        stats.current_thread_live_allocations,
        stats.current_thread_owned_segments,
        stats.cross_thread_reclaimed_blocks,
        stats.page_refills,
        stats.recycled_pages,
        stats.fresh_pages,
        stats.fresh_segments,
        stats.orphan_segments_adopted,
        stats.recycle_sweeps
    );
}

fn print_occupancy(phase: &str, stats: mnemosyne::MemoryStats) {
    for (class, occupancy) in stats.size_class_occupancy.into_iter().enumerate() {
        if occupancy.active_pages > 0 {
            println!(
                "{},{},{},{},{},{}",
                phase,
                class,
                occupancy.active_pages,
                occupancy.empty_pages,
                occupancy.live_allocations,
                occupancy.total_slots
            );
        }
    }
}

fn run_segment_eviction() -> Result<mnemosyne::MemoryStats, &'static str> {
    let mut segments = [core::ptr::null_mut::<mnemosyne_core::Segment>(); SEGMENT_EVICTION_ALLOCS];
    for segment in &mut segments {
        // Safety: every returned segment is stored and later released exactly
        // once by this function.
        *segment = unsafe {
            mnemosyne_arena::allocate_segment::<mnemosyne_backend::MemoryBackendWrapper>()
                .ok_or("segment allocation failed")?
        };
    }
    for segment in segments {
        // Safety: `segment` was allocated above and has not been deallocated.
        unsafe {
            mnemosyne_arena::deallocate_segment::<mnemosyne_backend::MemoryBackendWrapper>(segment);
        }
    }
    let stats = mnemosyne::memory_stats();
    if stats.retained_free_segments > stats.max_retained_free_segments {
        return Err("Retained free segments exceeded the maximum allowed limit");
    }
    Ok(stats)
}

fn purge_segment_cache() -> Result<mnemosyne::MemoryStats, &'static str> {
    // Safety: this command owns the benchmark process and intentionally clears
    // Mnemosyne's reusable segment pool before reading telemetry.
    unsafe {
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }
    let stats = mnemosyne::memory_stats();
    if stats.retained_free_segments != 0 {
        return Err("Retained free segments is not zero after purge");
    }
    Ok(stats)
}
