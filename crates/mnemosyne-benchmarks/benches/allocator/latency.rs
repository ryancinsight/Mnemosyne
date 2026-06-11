use core::alloc::GlobalAlloc;
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput};
use std::alloc::System;

#[cfg(jemalloc_available)]
use super::compat::bench_jemalloc;
use super::constants::{BATCH_ALLOCS, HUGE_LAYOUT, LARGE_LAYOUT, MEDIUM_LAYOUT, SMALL_LAYOUT};
use super::helpers::{
    alloc_dealloc, burst_alloc_dealloc, dealloc_only, require_allocated, AllocatedBlock,
};

pub fn bench_allocator_cycles(c: &mut Criterion) {
    let mut group = c.benchmark_group("Allocator cycle latency");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
        ("huge/2m", HUGE_LAYOUT),
    ] {
        group.throughput(Throughput::Bytes(layout.size() as u64));
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&mnemosyne::Mnemosyne, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("System", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&System, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&mimalloc::MiMalloc, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("RpMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&rpmalloc::RpMalloc, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { alloc_dealloc(&snmalloc_rs::SnMalloc, *layout) })
        });
        #[cfg(jemalloc_available)]
        {
            group.bench_with_input(BenchmarkId::new("Jemalloc", name), &layout, |b, layout| {
                // Safety: `layout` comes from the static valid benchmark layout table.
                b.iter(|| unsafe { alloc_dealloc(&bench_jemalloc::Jemalloc, *layout) })
            });
        }
    }
    group.finish();
}

pub fn bench_allocator_alloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("Allocator allocation latency");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
        ("huge/2m", HUGE_LAYOUT),
    ] {
        group.throughput(Throughput::Bytes(layout.size() as u64));
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            b.iter_batched(
                || (),
                |_| unsafe { AllocatedBlock::new(&mnemosyne::Mnemosyne, *layout, "alloc_only") },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("System", name), &layout, |b, layout| {
            b.iter_batched(
                || (),
                |_| unsafe { AllocatedBlock::new(&System, *layout, "alloc_only") },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            b.iter_batched(
                || (),
                |_| unsafe { AllocatedBlock::new(&mimalloc::MiMalloc, *layout, "alloc_only") },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("RpMalloc", name), &layout, |b, layout| {
            b.iter_batched(
                || (),
                |_| unsafe { AllocatedBlock::new(&rpmalloc::RpMalloc, *layout, "alloc_only") },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            b.iter_batched(
                || (),
                |_| unsafe { AllocatedBlock::new(&snmalloc_rs::SnMalloc, *layout, "alloc_only") },
                BatchSize::SmallInput,
            )
        });
        #[cfg(jemalloc_available)]
        {
            group.bench_with_input(BenchmarkId::new("Jemalloc", name), &layout, |b, layout| {
                b.iter_batched(
                    || (),
                    |_| unsafe {
                        AllocatedBlock::new(&bench_jemalloc::Jemalloc, *layout, "alloc_only")
                    },
                    BatchSize::SmallInput,
                )
            });
        }
    }
    group.finish();
}

pub fn bench_allocator_dealloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("Allocator deallocation latency");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
        ("huge/2m", HUGE_LAYOUT),
    ] {
        group.throughput(Throughput::Bytes(layout.size() as u64));
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            b.iter_batched(
                || unsafe {
                    require_allocated(mnemosyne::Mnemosyne.alloc(*layout), "dealloc_only")
                },
                |ptr| unsafe { dealloc_only(&mnemosyne::Mnemosyne, ptr, *layout) },
                BatchSize::SmallInput,
            );
        });
        group.bench_with_input(BenchmarkId::new("System", name), &layout, |b, layout| {
            b.iter_batched(
                || unsafe { require_allocated(System.alloc(*layout), "dealloc_only") },
                |ptr| unsafe { dealloc_only(&System, ptr, *layout) },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            b.iter_batched(
                || unsafe { require_allocated(mimalloc::MiMalloc.alloc(*layout), "dealloc_only") },
                |ptr| unsafe { dealloc_only(&mimalloc::MiMalloc, ptr, *layout) },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("RpMalloc", name), &layout, |b, layout| {
            b.iter_batched(
                || unsafe { require_allocated(rpmalloc::RpMalloc.alloc(*layout), "dealloc_only") },
                |ptr| unsafe { dealloc_only(&rpmalloc::RpMalloc, ptr, *layout) },
                BatchSize::SmallInput,
            )
        });
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            b.iter_batched(
                || unsafe {
                    require_allocated(snmalloc_rs::SnMalloc.alloc(*layout), "dealloc_only")
                },
                |ptr| unsafe { dealloc_only(&snmalloc_rs::SnMalloc, ptr, *layout) },
                BatchSize::SmallInput,
            )
        });
        #[cfg(jemalloc_available)]
        {
            group.bench_with_input(BenchmarkId::new("Jemalloc", name), &layout, |b, layout| {
                b.iter_batched(
                    || unsafe {
                        require_allocated(bench_jemalloc::Jemalloc.alloc(*layout), "dealloc_only")
                    },
                    |ptr| unsafe { dealloc_only(&bench_jemalloc::Jemalloc, ptr, *layout) },
                    BatchSize::SmallInput,
                )
            });
        }
    }
    group.finish();
}

pub fn bench_allocator_bursts(c: &mut Criterion) {
    let mut group = c.benchmark_group("Allocator burst retention");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
    ] {
        group.throughput(Throughput::Bytes((layout.size() * BATCH_ALLOCS) as u64));
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { burst_alloc_dealloc(&mnemosyne::Mnemosyne, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("System", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { burst_alloc_dealloc(&System, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { burst_alloc_dealloc(&mimalloc::MiMalloc, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("RpMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { burst_alloc_dealloc(&rpmalloc::RpMalloc, *layout) })
        });
        group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe { burst_alloc_dealloc(&snmalloc_rs::SnMalloc, *layout) })
        });
        #[cfg(jemalloc_available)]
        {
            group.bench_with_input(BenchmarkId::new("Jemalloc", name), &layout, |b, layout| {
                // Safety: `layout` comes from the static valid benchmark layout table.
                b.iter(|| unsafe { burst_alloc_dealloc(&bench_jemalloc::Jemalloc, *layout) })
            });
        }
    }
    group.finish();
}
