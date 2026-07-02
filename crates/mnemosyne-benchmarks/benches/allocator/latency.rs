use core::alloc::GlobalAlloc;
use criterion::{Criterion, Throughput};
use std::alloc::System;

#[cfg(jemalloc_available)]
use super::compat::bench_jemalloc;
use super::constants::{BATCH_ALLOCS, HUGE_LAYOUT, LARGE_LAYOUT, MEDIUM_LAYOUT, SMALL_LAYOUT};
use super::helpers::{
    AllocatedBlock, alloc_dealloc, bench_batched_case, bench_iter_case, burst_alloc_dealloc,
    dealloc_only, require_allocated, snmalloc_skips,
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
        // `cycle` is the measured routine; passing it as a generic `fn` item
        // lets each comparator monomorphize independently (zero dispatch cost).
        // Safety: `layout` comes from the static valid benchmark layout table.
        fn cycle<A: GlobalAlloc>(a: &A, layout: &core::alloc::Layout) {
            unsafe { alloc_dealloc(a, *layout) }
        }
        bench_iter_case(
            &mut group,
            "Mnemosyne",
            name,
            &mnemosyne::Mnemosyne,
            &layout,
            cycle,
        );
        bench_iter_case(&mut group, "System", name, &System, &layout, cycle);
        bench_iter_case(
            &mut group,
            "MiMalloc",
            name,
            &mimalloc::MiMalloc,
            &layout,
            cycle,
        );
        bench_iter_case(
            &mut group,
            "RpMalloc",
            name,
            &rpmalloc::RpMalloc,
            &layout,
            cycle,
        );
        if !snmalloc_skips(name) {
            bench_iter_case(
                &mut group,
                "SnMalloc",
                name,
                &snmalloc_rs::SnMalloc,
                &layout,
                cycle,
            );
        }
        #[cfg(jemalloc_available)]
        bench_iter_case(
            &mut group,
            "Jemalloc",
            name,
            &bench_jemalloc::Jemalloc,
            &layout,
            cycle,
        );
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
        // Setup is the empty `()`; the timed routine allocates one block via
        // `AllocatedBlock`, whose `Drop` frees it after the measurement.
        fn setup<A: GlobalAlloc>(_a: &A, _layout: &core::alloc::Layout) {}
        fn alloc_only<'a, A: GlobalAlloc>(
            a: &'a A,
            _state: (),
            layout: &core::alloc::Layout,
        ) -> AllocatedBlock<'a, A> {
            unsafe { AllocatedBlock::new(a, *layout, "alloc_only") }
        }
        bench_batched_case(
            &mut group,
            "Mnemosyne",
            name,
            &mnemosyne::Mnemosyne,
            &layout,
            setup,
            alloc_only,
        );
        bench_batched_case(
            &mut group, "System", name, &System, &layout, setup, alloc_only,
        );
        bench_batched_case(
            &mut group,
            "MiMalloc",
            name,
            &mimalloc::MiMalloc,
            &layout,
            setup,
            alloc_only,
        );
        bench_batched_case(
            &mut group,
            "RpMalloc",
            name,
            &rpmalloc::RpMalloc,
            &layout,
            setup,
            alloc_only,
        );
        if !snmalloc_skips(name) {
            bench_batched_case(
                &mut group,
                "SnMalloc",
                name,
                &snmalloc_rs::SnMalloc,
                &layout,
                setup,
                alloc_only,
            );
        }
        #[cfg(jemalloc_available)]
        bench_batched_case(
            &mut group,
            "Jemalloc",
            name,
            &bench_jemalloc::Jemalloc,
            &layout,
            setup,
            alloc_only,
        );
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
        // Setup allocates one block (untimed); the timed routine frees it.
        fn setup<A: GlobalAlloc>(a: &A, layout: &core::alloc::Layout) -> *mut u8 {
            unsafe { require_allocated(a.alloc(*layout), "dealloc_only") }
        }
        fn dealloc<A: GlobalAlloc>(a: &A, ptr: *mut u8, layout: &core::alloc::Layout) {
            unsafe { dealloc_only(a, ptr, *layout) }
        }
        bench_batched_case(
            &mut group,
            "Mnemosyne",
            name,
            &mnemosyne::Mnemosyne,
            &layout,
            setup,
            dealloc,
        );
        bench_batched_case(&mut group, "System", name, &System, &layout, setup, dealloc);
        bench_batched_case(
            &mut group,
            "MiMalloc",
            name,
            &mimalloc::MiMalloc,
            &layout,
            setup,
            dealloc,
        );
        bench_batched_case(
            &mut group,
            "RpMalloc",
            name,
            &rpmalloc::RpMalloc,
            &layout,
            setup,
            dealloc,
        );
        if !snmalloc_skips(name) {
            bench_batched_case(
                &mut group,
                "SnMalloc",
                name,
                &snmalloc_rs::SnMalloc,
                &layout,
                setup,
                dealloc,
            );
        }
        #[cfg(jemalloc_available)]
        bench_batched_case(
            &mut group,
            "Jemalloc",
            name,
            &bench_jemalloc::Jemalloc,
            &layout,
            setup,
            dealloc,
        );
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
        // Safety: `layout` comes from the static valid benchmark layout table.
        fn burst<A: GlobalAlloc>(a: &A, layout: &core::alloc::Layout) {
            unsafe { burst_alloc_dealloc(a, *layout) }
        }
        bench_iter_case(
            &mut group,
            "Mnemosyne",
            name,
            &mnemosyne::Mnemosyne,
            &layout,
            burst,
        );
        bench_iter_case(&mut group, "System", name, &System, &layout, burst);
        bench_iter_case(
            &mut group,
            "MiMalloc",
            name,
            &mimalloc::MiMalloc,
            &layout,
            burst,
        );
        bench_iter_case(
            &mut group,
            "RpMalloc",
            name,
            &rpmalloc::RpMalloc,
            &layout,
            burst,
        );
        bench_iter_case(
            &mut group,
            "SnMalloc",
            name,
            &snmalloc_rs::SnMalloc,
            &layout,
            burst,
        );
        #[cfg(jemalloc_available)]
        bench_iter_case(
            &mut group,
            "Jemalloc",
            name,
            &bench_jemalloc::Jemalloc,
            &layout,
            burst,
        );
    }
    group.finish();
}
