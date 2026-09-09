//! Purge, reset, and decay operations on the retained pools.

/// Purges the global segment pool for a specific backend, releasing all retained/cached segments back to the OS.
pub fn purge_generic<B: mnemosyne_arena::HasSegmentPool>() {
    // SAFETY: Purging the segment pool releases only free segments that are
    // no longer actively referenced by any thread allocator cache.
    unsafe {
        mnemosyne_arena::purge_segment_pool::<B>();
    }
}

/// Purges the global segment pool, releasing all retained/cached segments back to the OS.
pub fn purge() {
    purge_generic::<mnemosyne_backend::MemoryBackendWrapper>();
}

/// Asks the OS to drop the physical backing of every retained standard free
/// segment for a specific backend without removing them from the cache.
///
/// Use this as a lighter-weight RSS-reduction knob than `purge`: the
/// standard segment cache stays warm so subsequent small allocations skip the
/// OS mapping syscall, while the resident memory footprint of idle segments
/// drops to the kernel's demand-fault baseline. The separate huge-allocation
/// cache is not reset by this operation.
pub fn reset_generic<B: mnemosyne_arena::HasSegmentPool>() {
    // SAFETY: reset_segment_pool drains the retained pool, issues
    // page_reset on each segment's mapping, and pushes them back into
    // the cache; no segment is released or accessed by another path.
    unsafe {
        mnemosyne_arena::reset_segment_pool::<B>();
    }
}

/// Asks the OS to drop the physical backing of every retained standard free
/// segment without removing them from the cache.
///
/// The separate huge-allocation cache is not reset; use [`purge`] when both
/// cache families must be released.
pub fn reset() {
    reset_generic::<mnemosyne_backend::MemoryBackendWrapper>();
}

/// Purges the global segment pool while keeping `warm_threshold` committed
/// segments ready for immediate reuse.
///
/// Unlike [`purge`], which releases everything, this keeps the
/// `warm_threshold` most-recently-retained segments committed so that a
/// burst of allocations immediately following the purge avoids
/// `VirtualAlloc`/`mmap` round-trips.
///
/// Pass `P::SEGMENT_POOL_WARM_THRESHOLD` for policy-level guidance, or `0`
/// for the same behaviour as `purge`.
pub fn purge_lazy(warm_threshold: usize) {
    // SAFETY: purge_segment_pool_with_warm only touches segments the pool
    // owns exclusively.
    unsafe {
        mnemosyne_arena::purge_segment_pool_with_warm::<mnemosyne_backend::MemoryBackendWrapper>(
            warm_threshold,
        );
    }
}

/// Purges with `StandardPolicy::SEGMENT_POOL_WARM_THRESHOLD` warm segments kept.
///
/// A higher-level shortcut for `purge_lazy(StandardPolicy::SEGMENT_POOL_WARM_THRESHOLD)`.
pub fn purge_standard() {
    use mnemosyne_core::policy::AllocPolicy;
    purge_lazy(mnemosyne_core::policy::StandardPolicy::SEGMENT_POOL_WARM_THRESHOLD);
}

/// Triggers a manual background decay and defragmentation cycle across all active memory backends.
pub fn decay() {
    mnemosyne_decay::decay_step();
}
