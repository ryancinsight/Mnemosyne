use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion, Throughput};

use super::constants::{SMALL_LAYOUT, THREAD_ALLOCS, THREADS};
use super::workers::ThreadCycleWorkers;

trait ProfilerMode {
    const NAME: &'static str;

    fn prepare();
}

struct Disabled;

impl ProfilerMode for Disabled {
    const NAME: &'static str = "Disabled";

    fn prepare() {
        mnemosyne_prof::reset_profiler_for_testing();
    }
}

struct LeakDetector;

impl ProfilerMode for LeakDetector {
    const NAME: &'static str = "LeakDetector";

    fn prepare() {
        mnemosyne_prof::reset_profiler_for_testing();
        mnemosyne_prof::enable_leak_detector();
    }
}

fn bench_mode<M: ProfilerMode>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    allocator: &'static mnemosyne::Mnemosyne,
) {
    let workers = ThreadCycleWorkers::new(allocator, SMALL_LAYOUT);
    M::prepare();
    group.bench_function(M::NAME, |b| b.iter(|| workers.run()));
    drop(workers);
    mnemosyne_prof::reset_profiler_for_testing();
}

/// Measures the same persistent four-thread allocation workload with and
/// without the leak detector. Setup and worker teardown stay outside the timed
/// region so the comparison isolates profiler work on allocator callbacks.
pub fn bench_profiler_contention(c: &mut Criterion) {
    static MNEMOSYNE: mnemosyne::Mnemosyne = mnemosyne::Mnemosyne;

    let mut group = c.benchmark_group("Profiler contention/multithreaded small allocation cycles");
    group.throughput(Throughput::Elements((THREADS * THREAD_ALLOCS) as u64));
    bench_mode::<Disabled>(&mut group, &MNEMOSYNE);
    bench_mode::<LeakDetector>(&mut group, &MNEMOSYNE);
    group.finish();
}
