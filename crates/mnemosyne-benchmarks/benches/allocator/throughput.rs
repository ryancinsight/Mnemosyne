use core::alloc::GlobalAlloc;
use criterion::{black_box, BenchmarkId, Criterion, Throughput};

#[cfg(jemalloc_available)]
use super::compat::bench_jemalloc;
use super::constants::{HUGE_LAYOUT, LARGE_LAYOUT, MEDIUM_LAYOUT, SMALL_LAYOUT};
use super::helpers::{alloc_usable_dealloc, benchmark_failure, require_allocated, snmalloc_skips};

pub fn bench_usable_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("Usable size latency");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
        ("huge/2m", HUGE_LAYOUT),
    ] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::new("Mnemosyne", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe {
                alloc_usable_dealloc(&mnemosyne::Mnemosyne, *layout, |ptr| {
                    // Safety: `ptr` came from the Mnemosyne allocator above.
                    mnemosyne::usable_size(ptr)
                })
            })
        });
        group.bench_with_input(BenchmarkId::new("MiMalloc", name), &layout, |b, layout| {
            // Safety: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe {
                alloc_usable_dealloc(&mimalloc::MiMalloc, *layout, |ptr| {
                    // Safety: `ptr` came from the mimalloc allocator above.
                    mimalloc::MiMalloc.usable_size(ptr)
                })
            })
        });
        if !snmalloc_skips(name) {
            group.bench_with_input(BenchmarkId::new("SnMalloc", name), &layout, |b, layout| {
                // Safety: `layout` comes from the static valid benchmark layout table.
                b.iter(|| unsafe {
                    alloc_usable_dealloc(&snmalloc_rs::SnMalloc, *layout, |ptr| {
                        match snmalloc_rs::SnMalloc.usable_size(ptr) {
                            Some(size) => size,
                            None => {
                                benchmark_failure("alloc_usable_dealloc", "snmalloc returned None")
                            }
                        }
                    })
                })
            });
        }
        #[cfg(jemalloc_available)]
        {
            group.bench_with_input(BenchmarkId::new("Jemalloc", name), &layout, |b, layout| {
                // Safety: `layout` comes from the static valid benchmark layout table.
                b.iter(|| unsafe {
                    alloc_usable_dealloc(&bench_jemalloc::Jemalloc, *layout, |ptr| {
                        // Safety: `ptr` came from the jemalloc allocator above;
                        // the call is covered by the enclosing `unsafe` block.
                        bench_jemalloc::usable_size(ptr)
                    })
                })
            });
        }
    }
    group.finish();
}

pub fn bench_usable_size_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("Usable size query latency");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
        ("huge/2m", HUGE_LAYOUT),
    ] {
        group.throughput(Throughput::Elements(1));

        // Safety: `layout` comes from the static valid benchmark layout table.
        let mnemosyne_ptr =
            unsafe { require_allocated(mnemosyne::Mnemosyne.alloc(layout), "usable_size_query") };
        group.bench_with_input(
            BenchmarkId::new("Mnemosyne", name),
            &mnemosyne_ptr,
            |b, ptr| b.iter(|| unsafe { mnemosyne::usable_size(black_box(*ptr)) }),
        );
        // Safety: pointer was allocated by Mnemosyne for `layout` above.
        unsafe { mnemosyne::Mnemosyne.dealloc(mnemosyne_ptr, layout) };

        // Safety: `layout` comes from the static valid benchmark layout table.
        let mimalloc_ptr =
            unsafe { require_allocated(mimalloc::MiMalloc.alloc(layout), "usable_size_query") };
        group.bench_with_input(
            BenchmarkId::new("MiMalloc", name),
            &mimalloc_ptr,
            |b, ptr| b.iter(|| unsafe { mimalloc::MiMalloc.usable_size(black_box(*ptr)) }),
        );
        // Safety: pointer was allocated by MiMalloc for `layout` above.
        unsafe { mimalloc::MiMalloc.dealloc(mimalloc_ptr, layout) };

        if !snmalloc_skips(name) {
            // Safety: `layout` comes from the static valid benchmark layout table.
            let snmalloc_ptr = unsafe {
                require_allocated(snmalloc_rs::SnMalloc.alloc(layout), "usable_size_query")
            };
            group.bench_with_input(
                BenchmarkId::new("SnMalloc", name),
                &snmalloc_ptr,
                |b, ptr| {
                    b.iter(
                        || match snmalloc_rs::SnMalloc.usable_size(black_box(*ptr)) {
                            Some(size) => size,
                            None => {
                                benchmark_failure("usable_size_query", "snmalloc returned None")
                            }
                        },
                    )
                },
            );
            // Safety: pointer was allocated by SnMalloc for `layout` above.
            unsafe { snmalloc_rs::SnMalloc.dealloc(snmalloc_ptr, layout) };
        }

        #[cfg(jemalloc_available)]
        {
            // Safety: `layout` comes from the static valid benchmark layout table.
            let jemalloc_ptr = unsafe {
                require_allocated(bench_jemalloc::Jemalloc.alloc(layout), "usable_size_query")
            };
            group.bench_with_input(
                BenchmarkId::new("Jemalloc", name),
                &jemalloc_ptr,
                |b, ptr| b.iter(|| unsafe { bench_jemalloc::usable_size(black_box(*ptr)) }),
            );
            // Safety: pointer was allocated by Jemalloc for `layout` above.
            unsafe { bench_jemalloc::Jemalloc.dealloc(jemalloc_ptr, layout) };
        }
    }
    group.finish();
}
