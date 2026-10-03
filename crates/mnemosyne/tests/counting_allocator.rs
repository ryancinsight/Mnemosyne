//! Behavior of `mnemosyne::counting` under an installed `CountingAllocator`.
//!
//! This harness is its own test binary because it installs a global allocator,
//! which must not perturb the other integration harnesses. Each test reads
//! only its own thread's counts, so the cases are independent under both the
//! threaded libtest harness and one-process-per-test runners.
//!
//! Natively the wrapper counts `Mnemosyne`, the allocator a program ships
//! with. Under Miri it counts `System`: under Stacked Borrows a
//! Mnemosyne-backed program still reaches the facade's quarantined writes
//! (ADR 0012, MN-LOCAL-MIRI-UB), so a Mnemosyne-backed run could not tell a
//! wrapper defect from those. The
//! wrapper only forwards, so its aliasing, provenance and thread-teardown
//! behavior is the same for either inner allocator.

use core::alloc::{GlobalAlloc, Layout};
use std::alloc::System;
use std::hint::black_box;
use std::sync::{Arc, Barrier};
use std::thread;

use mnemosyne::counting::{AllocationDelta, CountingAllocator, measure};

#[cfg(miri)]
#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[cfg(not(miri))]
#[global_allocator]
static ALLOCATOR: CountingAllocator<mnemosyne::Mnemosyne> =
    CountingAllocator::new(mnemosyne::Mnemosyne);

/// Moves the allocator's per-thread initialization out of the window.
fn warm() {
    #[cfg(not(miri))]
    mnemosyne::warm_current_thread();
}

/// An allocator that satisfies no request, to pin that an unsatisfied request
/// is not counted as an allocation. No pointer ever comes from it, so it is
/// never asked to resize or free one.
struct Unsatisfiable;

// SAFETY: `alloc` returns null, which the contract permits for a request that
// cannot be satisfied and which transfers no ownership. The provided
// `alloc_zeroed` and `realloc` reach only `alloc`, so they return null too.
// `dealloc` is unreachable for lack of a pointer and does nothing.
unsafe impl GlobalAlloc for Unsatisfiable {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

/// `System`, except that it refuses every resize. A refused `realloc` leaves
/// the original block allocated, which the contract requires of the caller to
/// free with its original layout.
struct RefusesRealloc;

// SAFETY: `alloc`, `dealloc` and `alloc_zeroed` forward verbatim to `System`,
// which upholds the contract. `realloc` returns null without touching `ptr`,
// which the contract permits and which leaves the block allocated.
unsafe impl GlobalAlloc for RefusesRealloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `alloc`'s contract; forwarded verbatim.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller upholds `dealloc`'s contract; forwarded verbatim.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `alloc_zeroed`'s contract; forwarded
        // verbatim.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, _ptr: *mut u8, _layout: Layout, _new_size: usize) -> *mut u8 {
        core::ptr::null_mut()
    }
}

#[test]
fn window_without_heap_traffic_reads_empty() {
    warm();

    let (sum, delta) = measure(|| black_box(3_u64) + black_box(4_u64));

    assert_eq!(sum, 7);
    assert_eq!(delta, AllocationDelta::default());
}

#[test]
fn one_extra_allocation_reads_exactly_one() {
    warm();

    let (block, delta) = measure(|| black_box(Box::new([0_u8; 24])));

    assert_eq!(block.len(), 24);
    assert_eq!(
        delta,
        AllocationDelta {
            allocations: 1,
            bytes_allocated: 24,
            ..AllocationDelta::default()
        }
    );
}

#[test]
fn zeroed_allocation_is_counted() {
    warm();

    let (zeros, delta) = measure(|| black_box(vec![0_u8; 32]));

    assert_eq!(zeros.len(), 32);
    assert_eq!(delta.allocations, 1);
    assert_eq!(delta.bytes_allocated, 32);
}

#[test]
fn growing_realloc_is_counted_and_retained_bytes_stay_exact() {
    warm();
    let mut buffer = Vec::<u8>::with_capacity(16);

    let ((), delta) = measure(|| {
        buffer.reserve_exact(64);
        black_box(&mut buffer);
    });

    assert_eq!(buffer.capacity(), 64);
    assert_eq!(
        delta,
        AllocationDelta {
            reallocations: 1,
            bytes_allocated: 48,
            ..AllocationDelta::default()
        }
    );
    assert_eq!(delta.bytes_retained(), 48);
}

#[test]
fn shrinking_realloc_is_counted_as_released_bytes() {
    warm();
    let mut buffer = Vec::<u8>::with_capacity(64);

    let ((), delta) = measure(|| {
        buffer.shrink_to(16);
        black_box(&mut buffer);
    });

    assert_eq!(buffer.capacity(), 16);
    assert_eq!(
        delta,
        AllocationDelta {
            reallocations: 1,
            bytes_deallocated: 48,
            ..AllocationDelta::default()
        }
    );
    assert_eq!(delta.bytes_retained(), -48);
}

#[test]
fn deallocation_is_counted() {
    warm();
    let block = black_box(Box::new([0_u8; 40]));

    let ((), delta) = measure(|| drop(block));

    assert_eq!(
        delta,
        AllocationDelta {
            deallocations: 1,
            bytes_deallocated: 40,
            ..AllocationDelta::default()
        }
    );
    assert_eq!(delta.bytes_retained(), -40);
}

#[test]
fn balanced_window_retains_nothing() {
    warm();

    let ((), delta) = measure(|| drop(black_box(Vec::<u8>::with_capacity(128))));

    assert_eq!(delta.allocations, 1);
    assert_eq!(delta.deallocations, 1);
    assert_eq!(delta.bytes_allocated, 128);
    assert_eq!(delta.bytes_deallocated, 128);
    assert_eq!(delta.bytes_retained(), 0);
}

#[test]
fn allocations_on_another_thread_are_not_counted() {
    warm();
    // `thread::spawn` allocates on the spawning thread, so the worker starts
    // before the window and the two barriers, built before it too, bracket the
    // worker's allocation without allocating themselves.
    let start = Arc::new(Barrier::new(2));
    let done = Arc::new(Barrier::new(2));
    let worker = {
        let (start, done) = (Arc::clone(&start), Arc::clone(&done));
        thread::spawn(move || {
            start.wait();
            let ((), own) = measure(|| drop(black_box(Vec::<u8>::with_capacity(1024))));
            done.wait();
            own
        })
    };

    let ((), observed) = measure(|| {
        start.wait();
        done.wait();
    });
    let worker_delta = worker.join().expect("invariant: the worker does not panic");

    assert_eq!(observed, AllocationDelta::default());
    assert_eq!(
        worker_delta,
        AllocationDelta {
            allocations: 1,
            deallocations: 1,
            bytes_allocated: 1024,
            bytes_deallocated: 1024,
            ..AllocationDelta::default()
        }
    );
}

#[test]
fn nested_windows_compose() {
    warm();

    let (inner, outer) = measure(|| {
        let first = black_box(Box::new(1_u64));
        let (second, inner) = measure(|| black_box(Box::new(2_u64)));
        drop((first, second));
        inner
    });

    assert_eq!(inner.allocations, 1);
    assert_eq!(inner.deallocations, 0);
    assert_eq!(outer.allocations, 2);
    assert_eq!(outer.deallocations, 2);
}

#[test]
fn unsatisfied_allocations_are_not_counted() {
    let layout = Layout::new::<[u8; 16]>();
    let counting = CountingAllocator::new(Unsatisfiable);

    let (pointers, delta) = measure(|| {
        // SAFETY: `layout` has non-zero size, and a null result is the only
        // outcome `Unsatisfiable` produces, so nothing is dereferenced.
        let fresh = unsafe { counting.alloc(layout) };
        // SAFETY: as for `alloc`, with the zeroing entry point.
        let zeroed = unsafe { counting.alloc_zeroed(layout) };
        [fresh, zeroed]
    });

    assert!(pointers.iter().all(|pointer| pointer.is_null()));
    assert_eq!(delta, AllocationDelta::default());
}

#[test]
fn refused_realloc_is_not_counted_and_leaves_the_block_valid() {
    let layout = Layout::new::<[u8; 16]>();
    let counting = CountingAllocator::new(RefusesRealloc);
    // SAFETY: `layout` has non-zero size.
    let block = unsafe { counting.alloc(layout) };
    assert!(!block.is_null());

    let (grown, delta) = measure(|| {
        // SAFETY: `block` was returned by `counting.alloc(layout)` and is still
        // allocated; 32 is non-zero and valid for `layout`'s alignment.
        unsafe { counting.realloc(block, layout, 32) }
    });
    // SAFETY: a refused `realloc` leaves `block` allocated with `layout`, and
    // this is its only release.
    unsafe { counting.dealloc(block, layout) };

    assert!(grown.is_null());
    assert_eq!(delta, AllocationDelta::default());
}
