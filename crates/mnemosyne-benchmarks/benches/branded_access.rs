//! Paired measurements for Melinoe permit-mediated branded-cell reads.
//!
//! Measurement environment: the Windows hybrid-core development host described
//! by `allocator::host` (Core Ultra 9 285K); host preparation reports the actual
//! affinity and power-throttling outcome before samples. Allocation, scope
//! construction, and reclamation are outside the timed regions; each sample
//! measures one branded payload read through a thread-local or region permit.

use criterion::{Criterion, black_box, criterion_group};
use mnemosyne::{BrandedCell, MemoryBackendWrapper, StandardPolicy, branded_scope, sync_scope};

#[path = "allocator/host.rs"]
mod host;

fn benchmark_thread_local_read(c: &mut Criterion) {
    branded_scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let block = heap
            .alloc_init(&token, 41_u64)
            .expect("thread-local benchmark allocation failed");
        // SAFETY: `alloc_init` returned a live block containing an initialized
        // `u64`; the benchmark owns the only cell handle until reclamation.
        let cell = unsafe { BrandedCell::from_block(block) };

        c.bench_function("branded_cell/read/thread_local", |b| {
            b.iter(|| {
                let cell = black_box(cell);
                let permit = black_box(&token);
                black_box(*cell.borrow(permit))
            })
        });

        // SAFETY: the benchmark created no other cell handle or derived
        // reference that remains live after the timed region.
        heap.free(&mut token, unsafe { cell.into_block() });
    });
}

fn benchmark_sync_region_read(c: &mut Criterion) {
    sync_scope::<StandardPolicy, MemoryBackendWrapper, _, _>(|heap, mut token| {
        let block = heap
            .alloc_init(&token, 41_u64)
            .expect("sync-region benchmark allocation failed");
        // SAFETY: `alloc_init` returned a live block containing an initialized
        // `u64`; the benchmark owns the only cell handle until reclamation.
        let cell = unsafe { BrandedCell::from_block(block) };

        c.bench_function("branded_cell/read/sync_region", |b| {
            b.iter(|| {
                let cell = black_box(cell);
                let permit = black_box(&token);
                black_box(*cell.borrow(permit))
            })
        });

        // SAFETY: the benchmark created no other cell handle or derived
        // reference that remains live after the timed region.
        heap.free(&mut token, unsafe { cell.into_block() });
    });
}

criterion_group! {
    name = benches;
    config = criterion::Criterion::default()
        .sample_size(10)
        .warm_up_time(core::time::Duration::from_millis(100))
        .measurement_time(core::time::Duration::from_millis(500));
    targets = benchmark_thread_local_read, benchmark_sync_region_read
}

/// Starts host preparation before Criterion registers or samples the rows.
fn main() {
    eprintln!("branded_access: {}", host::prepare_measurement_host());
    benches();
    Criterion::default().configure_from_args().final_summary();
}
