use super::fixtures::MockBackend;
use super::super::*;
use crate::LocalAllocatorSelector;

/// Safety regression guard for the guard-free small-allocation fast path.
#[test]
fn unguarded_fast_path_rejects_reentrant_borrow() {
    let _guard = TEST_LOCK
        .lock()
        .expect("local allocator test lock was poisoned");

    let outer_saw_reentrant_none = MockBackend::with_allocator(|_outer| {
        // Inside the guarded borrow: is_allocating is set.
        // Safety: the probe closure performs no allocator re-entry.
        let reentrant = unsafe { MockBackend::with_allocator_unguarded(|_inner| 0xC0FFEE_usize) };
        reentrant.is_none()
    });
    assert_eq!(
        outer_saw_reentrant_none,
        Some(true),
        "unguarded fast path aliased a live guarded borrow instead of rejecting re-entry"
    );

    // With no guard held, the unguarded path is permitted and runs `f`.
    // Safety: the closure does not re-enter the allocator.
    let allowed = unsafe { MockBackend::with_allocator_unguarded(|_alloc| 7_usize) };
    assert_eq!(
        allowed,
        Some(7),
        "unguarded path must run the closure when no guard is held"
    );
}
