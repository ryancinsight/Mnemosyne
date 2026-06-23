use criterion::{black_box, BenchmarkId, Criterion, Throughput};
use std::alloc::System;

#[cfg(jemalloc_available)]
use super::compat::bench_jemalloc;
use super::constants::{
    CROSS_THREAD_ALLOCS, HUGE_LAYOUT, LARGE_LAYOUT, MEDIUM_LAYOUT, SATURATED_THREAD_ALLOCS,
    SMALL_LAYOUT, THREADS, THREAD_ALLOCS,
};
use super::helpers::benchmark_failure;
use super::workers::{HandoffWorker, ThreadCycleWorkers};

pub fn bench_cross_thread_free(c: &mut Criterion) {
    static MNEMOSYNE: mnemosyne::Mnemosyne = mnemosyne::Mnemosyne;
    static SYSTEM: System = System;
    static MIMALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;
    static RPMALLOC: rpmalloc::RpMalloc = rpmalloc::RpMalloc;
    static SNMALLOC: snmalloc_rs::SnMalloc = snmalloc_rs::SnMalloc;
    #[cfg(jemalloc_available)]
    static JEMALLOC: bench_jemalloc::Jemalloc = bench_jemalloc::Jemalloc;

    let mut group = c.benchmark_group("Cross-thread free handoff");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
        ("huge/2m", HUGE_LAYOUT),
    ] {
        let count = if layout.size() > 64 * 1024 {
            8 // Avoid high memory pressure for huge allocations
        } else {
            CROSS_THREAD_ALLOCS
        };
        group.throughput(Throughput::Elements(count as u64));
        let mnemosyne_worker = HandoffWorker::new(&MNEMOSYNE);
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            b.iter(|| mnemosyne_worker.alloc_then_handoff(*layout, count))
        });
        drop(mnemosyne_worker);

        let system_worker = HandoffWorker::new(&SYSTEM);
        group.bench_with_input(BenchmarkId::new("System", name), &layout, |b, layout| {
            b.iter(|| system_worker.alloc_then_handoff(*layout, count))
        });
        drop(system_worker);

        let mimalloc_worker = HandoffWorker::new(&MIMALLOC);
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            b.iter(|| mimalloc_worker.alloc_then_handoff(*layout, count))
        });
        drop(mimalloc_worker);

        let rpmalloc_worker = HandoffWorker::new(&RPMALLOC);
        group.bench_with_input(BenchmarkId::new("RpMalloc", name), &layout, |b, layout| {
            b.iter(|| rpmalloc_worker.alloc_then_handoff(*layout, count))
        });
        drop(rpmalloc_worker);

        #[cfg(not(all(windows, target_arch = "x86_64")))]
        let skip_snmalloc = false;
        #[cfg(all(windows, target_arch = "x86_64"))]
        let skip_snmalloc = name == "huge/2m";

        if !skip_snmalloc {
            let snmalloc_worker = HandoffWorker::new(&SNMALLOC);
            group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
                b.iter(|| snmalloc_worker.alloc_then_handoff(*layout, count))
            });
            drop(snmalloc_worker);
        }

        #[cfg(jemalloc_available)]
        {
            let jemalloc_worker = HandoffWorker::new(&JEMALLOC);
            group.bench_with_input(BenchmarkId::new("Jemalloc", name), &layout, |b, layout| {
                b.iter(|| jemalloc_worker.alloc_then_handoff(*layout, count))
            });
            drop(jemalloc_worker);
        }
    }

    let stats = mnemosyne::memory_stats();
    if stats.retained_free_segments > stats.max_retained_free_segments {
        benchmark_failure(
            "cross-thread free handoff",
            "retained free segments exceeded configured maximum",
        );
    }
    black_box(stats);
    group.finish();
}

pub fn bench_multithreaded_alloc(c: &mut Criterion) {
    static MNEMOSYNE: mnemosyne::Mnemosyne = mnemosyne::Mnemosyne;
    static SYSTEM: System = System;
    static MIMALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;
    static RPMALLOC: rpmalloc::RpMalloc = rpmalloc::RpMalloc;
    static SNMALLOC: snmalloc_rs::SnMalloc = snmalloc_rs::SnMalloc;
    #[cfg(jemalloc_available)]
    static JEMALLOC: bench_jemalloc::Jemalloc = bench_jemalloc::Jemalloc;

    {
        let mut group = c.benchmark_group("Threaded small allocation cycles");
        group.throughput(Throughput::Elements((THREADS * THREAD_ALLOCS) as u64));

        let mnemosyne_workers = ThreadCycleWorkers::new(&MNEMOSYNE, SMALL_LAYOUT);
        group.bench_function("Mnemosyne", |b| b.iter(|| mnemosyne_workers.run()));
        drop(mnemosyne_workers);

        let system_workers = ThreadCycleWorkers::new(&SYSTEM, SMALL_LAYOUT);
        group.bench_function("System", |b| b.iter(|| system_workers.run()));
        drop(system_workers);

        let mimalloc_workers = ThreadCycleWorkers::new(&MIMALLOC, SMALL_LAYOUT);
        group.bench_function("MiMalloc", |b| b.iter(|| mimalloc_workers.run()));
        drop(mimalloc_workers);

        let rpmalloc_workers = ThreadCycleWorkers::new(&RPMALLOC, SMALL_LAYOUT);
        group.bench_function("RpMalloc", |b| b.iter(|| rpmalloc_workers.run()));
        drop(rpmalloc_workers);

        let snmalloc_workers = ThreadCycleWorkers::new(&SNMALLOC, SMALL_LAYOUT);
        group.bench_function("SnMalloc", |b| b.iter(|| snmalloc_workers.run()));
        drop(snmalloc_workers);

        #[cfg(jemalloc_available)]
        {
            let jemalloc_workers = ThreadCycleWorkers::new(&JEMALLOC, SMALL_LAYOUT);
            group.bench_function("Jemalloc", |b| b.iter(|| jemalloc_workers.run()));
            drop(jemalloc_workers);
        }
        group.finish();
    }

    {
        let mut group = c.benchmark_group("Threaded medium allocation cycles");
        group.throughput(Throughput::Elements((THREADS * THREAD_ALLOCS) as u64));

        let mnemosyne_workers = ThreadCycleWorkers::new(&MNEMOSYNE, MEDIUM_LAYOUT);
        group.bench_function("Mnemosyne", |b| b.iter(|| mnemosyne_workers.run()));
        drop(mnemosyne_workers);

        let system_workers = ThreadCycleWorkers::new(&SYSTEM, MEDIUM_LAYOUT);
        group.bench_function("System", |b| b.iter(|| system_workers.run()));
        drop(system_workers);

        let mimalloc_workers = ThreadCycleWorkers::new(&MIMALLOC, MEDIUM_LAYOUT);
        group.bench_function("MiMalloc", |b| b.iter(|| mimalloc_workers.run()));
        drop(mimalloc_workers);

        let rpmalloc_workers = ThreadCycleWorkers::new(&RPMALLOC, MEDIUM_LAYOUT);
        group.bench_function("RpMalloc", |b| b.iter(|| rpmalloc_workers.run()));
        drop(rpmalloc_workers);

        let snmalloc_workers = ThreadCycleWorkers::new(&SNMALLOC, MEDIUM_LAYOUT);
        group.bench_function("SnMalloc", |b| b.iter(|| snmalloc_workers.run()));
        drop(snmalloc_workers);

        #[cfg(jemalloc_available)]
        {
            let jemalloc_workers = ThreadCycleWorkers::new(&JEMALLOC, MEDIUM_LAYOUT);
            group.bench_function("Jemalloc", |b| b.iter(|| jemalloc_workers.run()));
            drop(jemalloc_workers);
        }
        group.finish();
    }
}

pub fn bench_saturated_multithreaded_alloc(c: &mut Criterion) {
    static MNEMOSYNE: mnemosyne::Mnemosyne = mnemosyne::Mnemosyne;
    static SYSTEM: System = System;
    static MIMALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;
    static RPMALLOC: rpmalloc::RpMalloc = rpmalloc::RpMalloc;
    static SNMALLOC: snmalloc_rs::SnMalloc = snmalloc_rs::SnMalloc;
    #[cfg(jemalloc_available)]
    static JEMALLOC: bench_jemalloc::Jemalloc = bench_jemalloc::Jemalloc;

    let mut group = c.benchmark_group("Threaded saturated small allocation cycles");
    group.throughput(Throughput::Elements(
        (THREADS * SATURATED_THREAD_ALLOCS) as u64,
    ));

    let mnemosyne_workers = ThreadCycleWorkers::new(&MNEMOSYNE, SMALL_LAYOUT);
    group.bench_function("Mnemosyne", |b| {
        b.iter(|| mnemosyne_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(mnemosyne_workers);

    let system_workers = ThreadCycleWorkers::new(&SYSTEM, SMALL_LAYOUT);
    group.bench_function("System", |b| {
        b.iter(|| system_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(system_workers);

    let mimalloc_workers = ThreadCycleWorkers::new(&MIMALLOC, SMALL_LAYOUT);
    group.bench_function("MiMalloc", |b| {
        b.iter(|| mimalloc_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(mimalloc_workers);

    let rpmalloc_workers = ThreadCycleWorkers::new(&RPMALLOC, SMALL_LAYOUT);
    group.bench_function("RpMalloc", |b| {
        b.iter(|| rpmalloc_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(rpmalloc_workers);

    let snmalloc_workers = ThreadCycleWorkers::new(&SNMALLOC, SMALL_LAYOUT);
    group.bench_function("SnMalloc", |b| {
        b.iter(|| snmalloc_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
    });
    drop(snmalloc_workers);

    #[cfg(jemalloc_available)]
    {
        let jemalloc_workers = ThreadCycleWorkers::new(&JEMALLOC, SMALL_LAYOUT);
        group.bench_function("Jemalloc", |b| {
            b.iter(|| jemalloc_workers.run_with_iterations(SATURATED_THREAD_ALLOCS))
        });
        drop(jemalloc_workers);
    }

    group.finish();
}
