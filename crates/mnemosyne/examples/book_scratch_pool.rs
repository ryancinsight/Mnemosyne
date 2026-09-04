//! Temporary buffers through Mnemosyne's scratch pool.
//!
//! [`ScratchPool<T>`] provides a reusable pool of up to
//! [`MAX_POOL_SLOTS`] aligned buffers for a single element type.  The
//! pool is designed for `thread_local!` storage; it is `Send` but not `Sync`.
//! Construction is zero-allocation: the backing buffer is only mapped when
//! `with_scratch` is first called.
//!
//! Up to `MAX_POOL_SLOTS` (4) nested borrows are supported without growing the
//! pool, which covers recursive FFT twiddle computation, nested solver
//! residuals, and similar patterns common in kwavers and apollo.

extern crate mnemosyne;

use mnemosyne::scratch::{MAX_POOL_SLOTS, ScratchPool};

fn dot_product(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len());
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn main() {
    // Construct a pool locally and use it directly (not thread_local! here
    // to avoid the clippy::missing_const_for_thread_local false positive on
    // 1.97.0 — see ATLAS-MNEMOSYNE-CI-1).  In production callers store the
    // pool in thread_local! storage so it persists between calls.
    let f64_pool: ScratchPool<f64> = ScratchPool::new();
    let f32_pool: ScratchPool<f32> = ScratchPool::new();

    // Simple single-level scratch use: compute a dot product via a temp buffer.
    f64_pool.with_scratch(1024, |scratch| {
        for (i, slot) in scratch.iter_mut().enumerate() {
            *slot = i as f64;
        }
        let partial: f64 = scratch.iter().sum();
        println!(
            "sum of 0..1024 via scratch: {} (expected {})",
            partial,
            (0..1024usize).sum::<usize>() as f64,
        );
        assert_eq!(partial, (0..1024usize).sum::<usize>() as f64);
    });

    // Nested borrows: outer scratch holds the signal; inner holds a window.
    f64_pool.with_scratch(256, |signal| {
        for (i, s) in signal.iter_mut().enumerate() {
            *s = (i as f64).sin();
        }
        f64_pool.with_scratch(16, |window| {
            window.copy_from_slice(&signal[..16]);
            let dp = dot_product(window, &signal[..16]);
            println!("windowed dot-product: {dp:.6}");
            assert!(dp.is_finite());
        });
    });

    // F32 pool: same API, different element type.
    f32_pool.with_scratch(64, |buf| {
        buf.fill(1.0_f32);
        let total: f32 = buf.iter().sum();
        println!("f32 scratch sum: {total}");
        assert_eq!(total, 64.0_f32);
    });

    // Bounded provisioning retains the working set while making geometric
    // growth headroom reclaimable at a consumer-selected quiescent point.
    let bounded_pool: ScratchPool<u32> = ScratchPool::new();
    bounded_pool.with_scratch_bounded(1024, |_| {});
    bounded_pool.with_scratch_bounded(1025, |_| {});
    assert_eq!(bounded_pool.capacity(), 2048);
    let retained = bounded_pool.release();
    println!(
        "bounded release: capacity={} -> retained={}",
        2048, retained[0]
    );
    assert_eq!(retained[0], 1025);
    bounded_pool.reset();
    assert_eq!(bounded_pool.release()[0], 0);

    println!("MAX_POOL_SLOTS = {MAX_POOL_SLOTS} (max concurrent nested borrows)");
    println!("all scratch-pool assertions passed");
}
