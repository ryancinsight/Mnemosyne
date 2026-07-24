use criterion::{Criterion, Throughput, black_box};

use super::constants::SEGMENT_EVICTION_ALLOCS;
use super::failure::benchmark_failure;

#[inline(never)]
/// Exercises the segment cache retention and purge boundary.
///
/// # Safety
///
/// Every segment returned by the arena allocator is retained in the local
/// array and deallocated exactly once before the function returns.
unsafe fn segment_cache_eviction_cycle() {
    unsafe {
        let mut segments =
            [core::ptr::null_mut::<mnemosyne_core::Segment>(); SEGMENT_EVICTION_ALLOCS];
        for segment in &mut segments {
            // Safety: benchmark owns every returned segment pointer until it is
            // deallocated later in this function.
            *segment = match mnemosyne_arena::allocate_segment::<
                mnemosyne_backend::MemoryBackendWrapper,
            >() {
                Some(segment) => segment,
                None => benchmark_failure("segment cache eviction", "segment allocation failed"),
            };
        }
        black_box(&segments);
        for segment in segments {
            // Safety: each `segment` was allocated above and is deallocated exactly once.
            mnemosyne_arena::deallocate_segment::<mnemosyne_backend::MemoryBackendWrapper>(segment);
        }
        let stats =
            mnemosyne_arena::arena_memory_stats::<mnemosyne_backend::MemoryBackendWrapper>();
        if stats.retained_free_segments > stats.max_retained_free_segments {
            benchmark_failure(
                "segment cache eviction",
                "retained free segments exceeded configured maximum",
            );
        }
    }
}

pub fn bench_segment_cache_eviction(c: &mut Criterion) {
    // Safety: benchmark setup clears only Mnemosyne's reusable segment pool.
    unsafe {
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }

    let mut group = c.benchmark_group("Segment cache eviction");
    group.throughput(Throughput::Elements(SEGMENT_EVICTION_ALLOCS as u64));
    group.bench_function("Mnemosyne", |b| {
        // Safety: `segment_cache_eviction_cycle` owns every allocated segment.
        b.iter(|| unsafe { segment_cache_eviction_cycle() })
    });
    group.finish();

    // Safety: benchmark teardown clears only Mnemosyne's reusable segment pool.
    unsafe {
        mnemosyne_arena::purge_segment_pool::<mnemosyne_backend::MemoryBackendWrapper>();
    }
}
