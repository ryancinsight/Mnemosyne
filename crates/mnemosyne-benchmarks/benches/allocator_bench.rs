use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};

mod allocator;

use allocator::{
    bench_allocator_alloc, bench_allocator_bursts, bench_allocator_cycles, bench_allocator_dealloc,
    bench_cross_thread_free, bench_leak_detector_allocator_cycles, bench_multithreaded_alloc,
    bench_realloc, bench_saturated_multithreaded_alloc, bench_segment_cache_eviction,
    bench_usable_size, bench_usable_size_query,
};

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(100))
        .measurement_time(Duration::from_millis(500));
    targets =
        bench_allocator_cycles,
        bench_leak_detector_allocator_cycles,
        bench_allocator_alloc,
        bench_allocator_dealloc,
        bench_allocator_bursts,
        bench_usable_size,
        bench_usable_size_query,
        bench_realloc,
        bench_cross_thread_free,
        bench_multithreaded_alloc,
        bench_saturated_multithreaded_alloc,
        bench_segment_cache_eviction
}
criterion_main!(benches);
