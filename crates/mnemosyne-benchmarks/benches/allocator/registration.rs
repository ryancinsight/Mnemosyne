use criterion::measurement::WallTime;
use criterion::{BatchSize, BenchmarkGroup, BenchmarkId};

/// Registers one comparator column measured with `b.iter`.
///
/// The generic `alloc` reference monomorphizes per comparator, so the timed
/// region — `b.iter(|| routine(alloc, input))` — compiles identically to the
/// hand-written per-allocator body it replaces (zero dispatch cost). Only the
/// measured `routine` closure varies between call sites; the `BenchmarkId`,
/// input binding, and `b.iter` scaffolding live here once.
#[inline(always)]
pub fn bench_iter_case<A, I, R, O>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    alloc_name: &str,
    id: &str,
    alloc: &A,
    input: &I,
    routine: R,
) where
    A: std::alloc::GlobalAlloc,
    R: Fn(&A, &I) -> O,
{
    group.bench_with_input(BenchmarkId::new(alloc_name, id), input, |b, input| {
        b.iter(|| routine(alloc, input))
    });
}

/// Registers one comparator column measured with `b.iter_batched`
/// (`BatchSize::SmallInput`), splitting per-iteration `setup` from the timed
/// `routine`. As with [`bench_iter_case`], the generic `alloc` monomorphizes
/// per comparator so the timed region is byte-identical to the hand-written
/// body; the batched scaffolding is written once here.
#[inline(always)]
pub fn bench_batched_case<'a, A, I, S, R, T, O>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    alloc_name: &str,
    id: &str,
    alloc: &'a A,
    input: &I,
    setup: S,
    routine: R,
) where
    A: std::alloc::GlobalAlloc,
    S: Fn(&'a A, &I) -> T,
    R: Fn(&'a A, T, &I) -> O,
{
    group.bench_with_input(BenchmarkId::new(alloc_name, id), input, |b, input| {
        b.iter_batched(
            || setup(alloc, input),
            |state| routine(alloc, state, input),
            BatchSize::SmallInput,
        )
    });
}
