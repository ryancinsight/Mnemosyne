//! Request validation at the allocation entry points.

use super::*;

#[test]
fn thread_alloc_rejects_invalid_alignment_requests() {
    for &align in &[0usize, 3, 6, 12, SEGMENT_SIZE * 2] {
        let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(64, align) };
        assert!(
            ptr.is_null(),
            "invalid alignment {align} should be rejected"
        );
    }
}

#[test]
fn thread_alloc_rejects_zero_size_requests() {
    for &align in &[1usize, 8, 16, PAGE_SIZE] {
        let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(0, align) };
        assert!(ptr.is_null(), "zero-size allocation should be rejected");
    }
}

#[test]
fn thread_alloc_rejects_size_above_layout_bound() {
    let ptr =
        unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(MAX_ALLOC_SIZE + 1, 8) };
    assert!(
        ptr.is_null(),
        "above-MAX_ALLOC_SIZE thread_alloc returned {ptr:?}"
    );
}

#[test]
fn thread_alloc_layout_uses_layout_validated_fast_entry() {
    let ptr = unsafe { thread_alloc_layout::<StandardPolicy, MemoryBackendWrapper>(64, 8) };
    assert!(
        !ptr.is_null(),
        "Layout-validated thread_alloc fast entry returned null"
    );
    unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };

    let oversized = unsafe {
        thread_alloc_layout::<StandardPolicy, MemoryBackendWrapper>(64, SEGMENT_SIZE * 2)
    };
    assert!(
        oversized.is_null(),
        "Layout-validated oversized alignment returned {oversized:?}"
    );
}
