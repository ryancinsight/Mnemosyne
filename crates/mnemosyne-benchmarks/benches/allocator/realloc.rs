use core::alloc::{GlobalAlloc, Layout};
use criterion::{Criterion, Throughput};
use std::alloc::System;

use super::allocation::alloc_realloc_dealloc;
#[cfg(jemalloc_available)]
use super::compat::bench_jemalloc;
use super::constants::{
    HUGE_REALLOC_SRC_LAYOUT, LARGE_LAYOUT, LARGE_WITHIN_CLASS_LAYOUT, SMALL_LAYOUT,
    SMALL_WITHIN_CLASS_LAYOUT,
};
use super::platform::snmalloc_skips;
use super::registration::bench_iter_case;

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
        // Safety: inputs come from the static valid benchmark layout table.
        fn realloc<A: GlobalAlloc>(a: &A, input: &(Layout, usize)) {
            let (layout, new_size) = input;
            unsafe { alloc_realloc_dealloc(a, *layout, *new_size) }
        }
        let input = (layout, new_size);
        bench_iter_case(
            &mut group,
            "Mnemosyne",
            name,
            &mnemosyne::Mnemosyne,
            &input,
            realloc,
        );
        bench_iter_case(&mut group, "System", name, &System, &input, realloc);
        bench_iter_case(
            &mut group,
            "MiMalloc",
            name,
            &mimalloc::MiMalloc,
            &input,
            realloc,
        );
        bench_iter_case(
            &mut group,
            "RpMalloc",
            name,
            &rpmalloc::RpMalloc,
            &input,
            realloc,
        );
        if !snmalloc_skips(name) {
            bench_iter_case(
                &mut group,
                "SnMalloc",
                name,
                &snmalloc_rs::SnMalloc,
                &input,
                realloc,
            );
        }
        #[cfg(jemalloc_available)]
        bench_iter_case(
            &mut group,
            "Jemalloc",
            name,
            &bench_jemalloc::Jemalloc,
            &input,
            realloc,
        );
    }
    group.finish();
}
