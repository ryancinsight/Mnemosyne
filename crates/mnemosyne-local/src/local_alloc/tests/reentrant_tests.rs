use super::super::*;
use super::fixtures::MockBackend;
use crate::LocalAllocatorSelector;

/// Safety regression guard for the guard-free small-allocation fast path.
#[test]
fn unguarded_fast_path_rejects_reentrant_borrow() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let outer_saw_reentrant_none = MockBackend::with_allocator(|_outer| {
        // Inside the guarded borrow: is_allocating is set.
        // SAFETY: the probe closure performs no allocator re-entry.
        let reentrant = unsafe { MockBackend::with_allocator_unguarded(|_inner| 0xC0FFEE_usize) };
        reentrant.is_none()
    });
    assert_eq!(
        outer_saw_reentrant_none,
        Some(true),
        "unguarded fast path aliased a live guarded borrow instead of rejecting re-entry"
    );

    // With no guard held, the unguarded path is permitted and runs `f`.
    // SAFETY: the closure does not re-enter the allocator.
    let allowed = unsafe { MockBackend::with_allocator_unguarded(|_alloc| 7_usize) };
    assert_eq!(
        allowed,
        Some(7),
        "unguarded path must run the closure when no guard is held"
    );
}

/// Cold-branch guard: the unguarded path's first-touch branch must hand the
/// callee the freshly cached allocator pointer, not the null cache value that
/// routed it into that branch.
///
/// The test above cannot reach this branch: its guarded `with_allocator` call
/// populates the thread's cache cell first, so every later unguarded call on
/// that thread takes the hot branch. A fresh thread starts with an empty cell,
/// which is the only way the cold branch runs. Passing the null pointer
/// dereferences it while projecting the re-entry flag, so a regression fails
/// this test by killing its process — which nextest reports per test.
#[test]
fn unguarded_cold_branch_uses_the_cached_allocator_pointer() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let observed = std::thread::spawn(|| {
        // SAFETY: first allocator touch on this thread, so no borrow is live
        // and the closure performs no re-entry.
        unsafe { MockBackend::with_allocator_unguarded(|_alloc| 0xBEEF_usize) }
    })
    .join()
    .expect("cold-branch probe thread died: first-touch unguarded path is unsound");

    assert_eq!(
        observed,
        Some(0xBEEF),
        "unguarded cold branch must initialize the slot and run the closure"
    );
}
