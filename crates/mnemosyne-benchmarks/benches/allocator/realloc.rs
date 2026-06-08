use criterion::{BenchmarkId, Criterion, Throughput};
use std::alloc::System;

#[cfg(jemalloc_available)]
use super::compat::bench_jemalloc;
use super::constants::{
    HUGE_REALLOC_SRC_LAYOUT, LARGE_LAYOUT, LARGE_WITHIN_CLASS_LAYOUT, SMALL_LAYOUT,
    SMALL_WITHIN_CLASS_LAYOUT,
};
use super::helpers::alloc_realloc_dealloc;

pub fn bench_realloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("Realloc latency");
    for (name, layout, new_size) in [
        ("within_class_24_to_32", SMALL_WITHIN_CLASS_LAYOUT, 32usize),
        ("cross_class_32_to_64", SMALL_LAYOUT, 64usize),
        (
            "within_class_6k_to_8k",
            LARGE_WITHIN_CLASS_LAYOUT,
            8192usize,
        ),
        ("cross_class_8k_to_16k", LARGE_LAYOUT, 16384usize),
        (
            "huge_shrink_4m_to_2m",
            HUGE_REALLOC_SRC_LAYOUT,
            2 * 1024 * 1024usize,
        ),
    ] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("Mnemosyne", name),
            &(layout, new_size),
            |b, (layout, new_size)| {
                // Safety: inputs come from the static valid benchmark layout table.
                b.iter(|| unsafe {
                    alloc_realloc_dealloc(&mnemosyne::Mnemosyne, *layout, *new_size)
                })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("System", name),
            &(layout, new_size),
            |b, (layout, new_size)| {
                // Safety: inputs come from the static valid benchmark layout table.
                b.iter(|| unsafe { alloc_realloc_dealloc(&System, *layout, *new_size) })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("MiMalloc", name),
            &(layout, new_size),
            |b, (layout, new_size)| {
                // Safety: inputs come from the static valid benchmark layout table.
                b.iter(|| unsafe { alloc_realloc_dealloc(&mimalloc::MiMalloc, *layout, *new_size) })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("RpMalloc", name),
            &(layout, new_size),
            |b, (layout, new_size)| {
                // Safety: inputs come from the static valid benchmark layout table.
                b.iter(|| unsafe { alloc_realloc_dealloc(&rpmalloc::RpMalloc, *layout, *new_size) })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("SnMalloc", name),
            &(layout, new_size),
            |b, (layout, new_size)| {
                // Safety: inputs come from the static valid benchmark layout table.
                b.iter(|| unsafe {
                    alloc_realloc_dealloc(&snmalloc_rs::SnMalloc, *layout, *new_size)
                })
            },
        );
        #[cfg(jemalloc_available)]
        {
            group.bench_with_input(
                BenchmarkId::new("Jemalloc", name),
                &(layout, new_size),
                |b, (layout, new_size)| {
                    // Safety: inputs come from the static valid benchmark layout table.
                    b.iter(|| unsafe {
                        alloc_realloc_dealloc(&bench_jemalloc::Jemalloc, *layout, *new_size)
                    })
                },
            );
        }
    }
    group.finish();
}
