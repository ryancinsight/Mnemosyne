use criterion::{Criterion, criterion_group};

mod allocator;

use allocator::{
    bench_allocator_alloc, bench_allocator_bursts, bench_allocator_cycles, bench_allocator_dealloc,
    bench_cross_thread_free, bench_leak_detector_allocator_cycles, bench_multithreaded_alloc,
    bench_profiler_contention, bench_realloc, bench_saturated_multithreaded_alloc,
    bench_segment_cache_eviction, bench_usable_size, bench_usable_size_query, default_criterion,
    prepare_measurement_host,
};

criterion_group! {
    name = benches;
    config = default_criterion();
    targets =
        bench_allocator_cycles,
        bench_leak_detector_allocator_cycles,
        bench_profiler_contention,
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

/// `criterion_main!` expanded so host preparation runs before the first sample
/// is taken. The two statements after it are exactly what the macro generates.
fn main() {
    eprintln!("allocator_bench: {}", prepare_measurement_host());
    benches();
    Criterion::default().configure_from_args().final_summary();
}
