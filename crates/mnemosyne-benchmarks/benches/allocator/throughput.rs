use core::alloc::GlobalAlloc;
use criterion::{Criterion, Throughput, black_box};

use super::allocation::alloc_usable_dealloc;
#[cfg(jemalloc_available)]
use super::compat::bench_jemalloc;
use super::constants::{HUGE_LAYOUT, LARGE_LAYOUT, MEDIUM_LAYOUT, SMALL_LAYOUT};
#[cfg(feature = "snmalloc")]
use super::failure::benchmark_failure;
use super::failure::require_allocated;
#[cfg(feature = "snmalloc")]
use super::platform::snmalloc_skips;
use super::registration::bench_column;

pub fn bench_usable_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("Usable size latency");
    for (name, layout) in [
        ("small/32", SMALL_LAYOUT),
        ("medium/1024", MEDIUM_LAYOUT),
        ("large/8192", LARGE_LAYOUT),
        ("huge/2m", HUGE_LAYOUT),
    ] {
        group.throughput(Throughput::Elements(1));
        bench_column(&mut group, "Mnemosyne", name, &layout, |b, layout| {
            // SAFETY: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe {
                alloc_usable_dealloc(&mnemosyne::Mnemosyne, *layout, |ptr| {
                    // SAFETY: `ptr` came from the Mnemosyne allocator above.
                    mnemosyne::usable_size(ptr)
                })
            })
        });
        bench_column(&mut group, "MiMalloc", name, &layout, |b, layout| {
            // SAFETY: `layout` comes from the static valid benchmark layout table.
            b.iter(|| unsafe {
                alloc_usable_dealloc(&mimalloc::MiMalloc, *layout, |ptr| {
                    // SAFETY: `ptr` came from the mimalloc allocator above.
                    mimalloc::MiMalloc.usable_size(ptr)
                })
            })
        });
        #[cfg(feature = "snmalloc")]
        if !snmalloc_skips(name) {
            bench_column(&mut group, "SnMalloc", name, &layout, |b, layout| {
                // SAFETY: `layout` comes from the static valid benchmark layout table.
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
            bench_column(&mut group, "Jemalloc", name, &layout, |b, layout| {
                // SAFETY: `layout` comes from the static valid benchmark layout table.
                b.iter(|| unsafe {
                    alloc_usable_dealloc(&bench_jemalloc::Jemalloc, *layout, |ptr| {
                        // SAFETY: `ptr` came from the jemalloc allocator above;
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

        // SAFETY: `layout` comes from the static valid benchmark layout table.
        let mnemosyne_ptr =
            unsafe { require_allocated(mnemosyne::Mnemosyne.alloc(layout), "usable_size_query") };
        bench_column(&mut group, "Mnemosyne", name, &mnemosyne_ptr, |b, ptr| {
            b.iter(|| unsafe { mnemosyne::usable_size(black_box(*ptr)) })
        });
        // SAFETY: pointer was allocated by Mnemosyne for `layout` above.
        unsafe { mnemosyne::Mnemosyne.dealloc(mnemosyne_ptr, layout) };

        // SAFETY: `layout` comes from the static valid benchmark layout table.
        let mimalloc_ptr =
            unsafe { require_allocated(mimalloc::MiMalloc.alloc(layout), "usable_size_query") };
        bench_column(&mut group, "MiMalloc", name, &mimalloc_ptr, |b, ptr| {
            b.iter(|| unsafe { mimalloc::MiMalloc.usable_size(black_box(*ptr)) })
        });
        // SAFETY: pointer was allocated by MiMalloc for `layout` above.
        unsafe { mimalloc::MiMalloc.dealloc(mimalloc_ptr, layout) };

        #[cfg(feature = "snmalloc")]
        if !snmalloc_skips(name) {
            // SAFETY: `layout` comes from the static valid benchmark layout table.
            let snmalloc_ptr = unsafe {
                require_allocated(snmalloc_rs::SnMalloc.alloc(layout), "usable_size_query")
            };
            bench_column(&mut group, "SnMalloc", name, &snmalloc_ptr, |b, ptr| {
                b.iter(
                    || match snmalloc_rs::SnMalloc.usable_size(black_box(*ptr)) {
                        Some(size) => size,
                        None => benchmark_failure("usable_size_query", "snmalloc returned None"),
                    },
                )
            });
            // SAFETY: pointer was allocated by SnMalloc for `layout` above.
            unsafe { snmalloc_rs::SnMalloc.dealloc(snmalloc_ptr, layout) };
        }

        #[cfg(jemalloc_available)]
        {
            // SAFETY: `layout` comes from the static valid benchmark layout table.
            let jemalloc_ptr = unsafe {
                require_allocated(bench_jemalloc::Jemalloc.alloc(layout), "usable_size_query")
            };
            bench_column(&mut group, "Jemalloc", name, &jemalloc_ptr, |b, ptr| {
                b.iter(|| unsafe { bench_jemalloc::usable_size(black_box(*ptr)) })
            });
            // SAFETY: pointer was allocated by Jemalloc for `layout` above.
            unsafe { bench_jemalloc::Jemalloc.dealloc(jemalloc_ptr, layout) };
        }
    }
    group.finish();
}
