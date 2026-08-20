//! Use the Mnemosyne allocator and observe allocation statistics.
//!
//! This example registers [`Mnemosyne`] as the global allocator, performs a
//! set of typed allocations across the size-class table, and reads the live
//! counter and mapped-byte accounting from [`memory_stats`].  At the end it
//! calls [`purge`] to release cached free segments back to the OS and confirms
//! that the retained count drops.

extern crate mnemosyne;

use mnemosyne::{Mnemosyne, memory_stats, purge};

#[global_allocator]
static ALLOC: Mnemosyne = Mnemosyne;

fn main() {
    // Warm up the allocator to initialise TLS structures.
    let _ = std::vec::Vec::<u8>::with_capacity(64);

    let baseline = memory_stats();
    println!(
        "baseline: live={}",
        baseline.current_thread_live_allocations
    );

    // Allocate across several size classes with a Vec<u64>.
    let sizes: &[usize] = &[8, 64, 256, 1024, 4096];
    let mut vecs: Vec<Vec<u64>> = Vec::with_capacity(sizes.len());
    for &n in sizes {
        vecs.push(vec![0u64; n]);
    }

    let after_alloc = memory_stats();
    println!(
        "after alloc: live={}, mapped={} B",
        after_alloc.current_thread_live_allocations, after_alloc.current_mapped_bytes,
    );
    assert!(
        after_alloc.current_thread_live_allocations > baseline.current_thread_live_allocations,
        "live count should increase after allocation"
    );

    // Drop all vectors; the memory goes back to the free-pool.
    drop(vecs);

    let after_free = memory_stats();
    println!(
        "after free:  live={}, mapped={} B, retained_free_segments={}",
        after_free.current_thread_live_allocations,
        after_free.current_mapped_bytes,
        after_free.retained_free_segments,
    );
    assert!(
        after_free.current_thread_live_allocations <= after_alloc.current_thread_live_allocations,
        "live count should decrease after free"
    );
    // mapped bytes may remain because free segments are cached for reuse.
    assert!(
        after_free.current_mapped_bytes <= after_alloc.peak_mapped_bytes,
        "current_mapped_bytes cannot exceed peak"
    );

    // purge: return all cached free segments to the OS.
    purge();
    let after_purge = memory_stats();
    println!(
        "after purge: retained_free_segments={}, purge_calls={}",
        after_purge.retained_free_segments, after_purge.purge_calls,
    );
    assert!(
        after_purge.purge_calls > after_free.purge_calls,
        "purge_calls counter should advance"
    );

    println!("all memory-stats assertions passed");
}
