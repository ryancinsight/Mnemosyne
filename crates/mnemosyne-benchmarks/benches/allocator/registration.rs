use criterion::measurement::WallTime;
use criterion::{BatchSize, Bencher, BenchmarkGroup, BenchmarkId};

use super::measurement::configure_column;

/// Registers one comparator column whose timed body is written at the call
/// site, applying that column's measurement budget first.
///
/// Every column goes through this function (or one of the two `_case` helpers
/// below, which call it in turn) because Criterion's group configuration is
/// stateful: a column registered directly would inherit whichever budget the
/// previously registered column left behind.
pub fn bench_column<I, F>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    column: &str,
    id: &str,
    input: &I,
    routine: F,
) where
    F: FnMut(&mut Bencher<'_, WallTime>, &I),
{
    configure_column(group, column);
    group.bench_with_input(BenchmarkId::new(column, id), input, routine);
}

/// Registers a column that is the group's only axis, so the column name is the
/// whole benchmark id. Otherwise identical to [`bench_column`].
pub fn bench_sole_column<F>(group: &mut BenchmarkGroup<'_, WallTime>, column: &str, routine: F)
where
    F: FnMut(&mut Bencher<'_, WallTime>),
{
    configure_column(group, column);
    group.bench_function(column, routine);
}

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
    bench_column(group, alloc_name, id, input, |b, input| {
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
    bench_column(group, alloc_name, id, input, |b, input| {
        b.iter_batched(
            || setup(alloc, input),
            |state| routine(alloc, state, input),
            BatchSize::SmallInput,
        )
    });
}
