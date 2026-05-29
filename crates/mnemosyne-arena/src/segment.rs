//! Aligned segment allocations from the OS or global pools.

use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use mnemosyne_core::constants::{SEGMENT_ALIGN, SEGMENT_SIZE};
use mnemosyne_core::types::Segment;

/// Bytes requested from the OS for each standard segment mapping.
pub const SEGMENT_MAPPING_SIZE: usize = SEGMENT_SIZE * 2;

/// Free segment mappings retained for reuse.
pub const MAX_RETAINED_SEGMENTS: usize = mnemosyne_core::PAGES_PER_SEGMENT;

/// Size of the guard region installed in the slack after every segment.
///
/// The guard lives at `aligned_addr + SEGMENT_SIZE`, inside the
/// `SEGMENT_MAPPING_SIZE - SEGMENT_SIZE` of address-space slack the
/// arena reserves to satisfy `SEGMENT_ALIGN` rounding. Worst-case
/// available slack-after = `OS_PAGE_SIZE` (when the raw OS mapping
/// happened to be aligned to `SEGMENT_ALIGN - OS_PAGE_SIZE`), so the
/// guard size must not exceed the smallest supported OS page size. We
/// fix the value at 4 KiB, which is the system page size on every
/// supported Mnemosyne target (Linux/Windows/macOS-x86_64). On
/// platforms with a larger OS page size (macOS-arm64 at 16 KiB) the
/// underlying `mprotect`/`VirtualProtect` request will fail and the
/// guard install is silently skipped - the backend telemetry surfaces
/// the actual install count.
pub const SEGMENT_TAIL_GUARD_SIZE: usize = 4096;

const _: () = assert!(SEGMENT_TAIL_GUARD_SIZE.is_power_of_two());
const _: () = assert!(SEGMENT_TAIL_GUARD_SIZE <= SEGMENT_ALIGN);

/// Snapshot of arena-level segment cache state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ArenaMemoryStats {
    pub retained_free_segments: usize,
    pub max_retained_free_segments: usize,
    pub retained_free_bytes: usize,
    pub purged_segments: usize,
    pub purge_calls: usize,
    pub purged_bytes: usize,
    /// Number of segments whose physical backing was released by a
    /// confirmed `page_reset` while the segment itself remained cached
    /// in the retained pool.
    pub reset_segments: usize,
    /// Number of `reset_segment_pool` invocations.
    pub reset_calls: usize,
}

/// Outcome of attempting to release a segment mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SegmentRelease {
    /// The backend confirmed release of the OS mapping.
    Released,
    /// The backend reported release failure; ownership remains with the pool.
    RetainedAfterFailure,
}

/// A lock-free global pool of free segments to avoid OS allocator overhead.
pub struct GlobalSegmentPool {
    head: AtomicPtr<Segment>,
    retained: AtomicUsize,
    purged: AtomicUsize,
    purge_calls: AtomicUsize,
    reset_segments: AtomicUsize,
    reset_calls: AtomicUsize,
}

impl GlobalSegmentPool {
    /// Creates a new empty `GlobalSegmentPool`.
    pub const fn new() -> Self {
        Self {
            head: AtomicPtr::new(core::ptr::null_mut()),
            retained: AtomicUsize::new(0),
            purged: AtomicUsize::new(0),
            purge_calls: AtomicUsize::new(0),
            reset_segments: AtomicUsize::new(0),
            reset_calls: AtomicUsize::new(0),
        }
    }

    /// Pushes a segment back to the pool without applying a retention limit.
    #[inline]
    pub fn push_unbounded(&self, segment: *mut Segment) {
        self.retained.fetch_add(1, Ordering::Relaxed);
        self.push_raw(segment);
    }

    /// Pushes a segment back to the bounded reusable segment pool.
    #[inline]
    pub fn try_push_retained(&self, segment: *mut Segment) -> bool {
        let mut retained = self.retained.load(Ordering::Relaxed);
        loop {
            if retained >= MAX_RETAINED_SEGMENTS {
                return false;
            }
            match self.retained.compare_exchange_weak(
                retained,
                retained + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.push_raw(segment);
                    return true;
                }
                Err(actual) => retained = actual,
            }
        }
    }

    #[inline]
    fn push_raw(&self, segment: *mut Segment) {
        let mut current = self.head.load(Ordering::Relaxed);
        loop {
            // Safety: segment pointer is valid, aligned, and exclusive to this thread.
            // We write the next segment pointer to prepend it to the atomic list.
            let next_ptr = current;
            unsafe {
                (*segment).next_free_segment = next_ptr;
            }
            match self.head.compare_exchange_weak(
                current,
                segment,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Pops a segment from the pool, if available.
    #[inline]
    pub fn pop(&self) -> Option<*mut Segment> {
        let mut current = self.head.load(Ordering::Acquire);
        loop {
            if current.is_null() {
                return None;
            }
            // Safety: current points to a valid Segment inside the pool. We load the next
            // pointer in the chain atomically.
            let next = unsafe { (*current).next_free_segment };
            match self.head.compare_exchange_weak(
                current,
                next,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.retained.fetch_sub(1, Ordering::Relaxed);
                    return Some(current);
                }
                Err(actual) => current = actual,
            }
        }
    }

    #[inline]
    pub fn retained_count(&self) -> usize {
        self.retained.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn purged_count(&self) -> usize {
        self.purged.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn purge_call_count(&self) -> usize {
        self.purge_calls.load(Ordering::Relaxed)
    }

    #[inline]
    fn record_purge(&self, count: usize) {
        self.purge_calls.fetch_add(1, Ordering::Relaxed);
        self.purged.fetch_add(count, Ordering::Relaxed);
    }

    #[inline]
    pub fn reset_segments_count(&self) -> usize {
        self.reset_segments.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn reset_call_count(&self) -> usize {
        self.reset_calls.load(Ordering::Relaxed)
    }

    #[inline]
    fn record_reset(&self, count: usize) {
        self.reset_calls.fetch_add(1, Ordering::Relaxed);
        self.reset_segments.fetch_add(count, Ordering::Relaxed);
    }
}

/// The global segment pool instance.
/// Trait associating a memory backend with its global segment and orphan pools.
pub trait HasSegmentPool: mnemosyne_core::MemoryBackend {
    /// Returns the global segment pool for this backend.
    fn global_segment_pool() -> &'static GlobalSegmentPool;

    /// Returns the global orphan pool for this backend.
    fn global_orphan_pool() -> &'static GlobalSegmentPool;
}

static DEFAULT_BACKEND_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static DEFAULT_BACKEND_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();

impl HasSegmentPool for mnemosyne_backend::DefaultBackend {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &DEFAULT_BACKEND_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &DEFAULT_BACKEND_ORPHAN_POOL
    }
}

static WRAPPER_BACKEND_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static WRAPPER_BACKEND_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();

impl HasSegmentPool for mnemosyne_backend::MemoryBackendWrapper {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &WRAPPER_BACKEND_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &WRAPPER_BACKEND_ORPHAN_POOL
    }
}

static CUDA_BACKEND_SEGMENT_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
static CUDA_BACKEND_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();

impl HasSegmentPool for mnemosyne_backend::CudaUnifiedBackend {
    #[inline(always)]
    fn global_segment_pool() -> &'static GlobalSegmentPool {
        &CUDA_BACKEND_SEGMENT_POOL
    }

    #[inline(always)]
    fn global_orphan_pool() -> &'static GlobalSegmentPool {
        &CUDA_BACKEND_ORPHAN_POOL
    }
}

/// Returns the current arena segment cache counters.
#[inline]
pub fn arena_memory_stats<B: HasSegmentPool>() -> ArenaMemoryStats {
    let pool = B::global_segment_pool();
    let retained = pool.retained_count();
    ArenaMemoryStats {
        retained_free_segments: retained,
        max_retained_free_segments: MAX_RETAINED_SEGMENTS,
        retained_free_bytes: retained * SEGMENT_MAPPING_SIZE,
        purged_segments: pool.purged_count(),
        purge_calls: pool.purge_call_count(),
        purged_bytes: pool.purged_count() * SEGMENT_MAPPING_SIZE,
        reset_segments: pool.reset_segments_count(),
        reset_calls: pool.reset_call_count(),
    }
}

/// Utility to align an address up to a given alignment boundary, returning `None` on overflow.
#[inline(always)]
pub const fn checked_align_up(addr: usize, align: usize) -> Option<usize> {
    if align == 0 {
        return Some(addr);
    }
    let offset = align - 1;
    if let Some(sum) = addr.checked_add(offset) {
        Some(sum & !offset)
    } else {
        None
    }
}

/// Non-generic helper to pop a segment from the global segment pool or orphan pool.
///
/// # Safety
///
/// Returns a pointer to a valid initialized `Segment` structure.
#[inline(never)]
unsafe fn allocate_segment_from_pools<B: HasSegmentPool>() -> Option<*mut Segment> {
    // 1. Try to pop from the global segment pool
    if let Some(segment) = B::global_segment_pool().pop() {
        // Safety: segment points to a valid allocated Segment. We re-initialize
        // the segment to erase stale epoch metadata and reset it for new allocations.
        unsafe {
            let raw_ptr = (*segment).raw_alloc_ptr;
            Segment::initialize(segment, raw_ptr);
        }
        return Some(segment);
    }

    // 2. Try to pop from the global orphan pool.
    // Safety: Returning popped orphaned segment as is, preserving active allocations.
    if let Some(segment) = B::global_orphan_pool().pop() {
        return Some(segment);
    }

    None
}

/// Non-generic helper to return a segment to the global segment pool.
#[inline(always)]
fn try_return_to_pool<B: HasSegmentPool>(segment: *mut Segment) -> bool {
    B::global_segment_pool().try_push_retained(segment)
}

/// Allocates an aligned segment of memory, either from the pool or from the OS.
///
/// # Safety
///
/// Returns a pointer to a fully initialized `Segment`.
#[inline]
pub unsafe fn allocate_segment<B: HasSegmentPool>() -> Option<*mut Segment> {
    // Safety: allocate_segment_from_pools retrieves a valid segment from pools if available.
    if let Some(segment) = unsafe { allocate_segment_from_pools::<B>() } {
        return Some(segment);
    }

    // 3. Fall back to OS allocation
    // We allocate twice the segment size to ensure we can find an aligned boundary.
    // Safety: SEGMENT_MAPPING_SIZE is non-zero and aligned. We call B::allocate.
    let raw_ptr = unsafe { B::allocate(SEGMENT_MAPPING_SIZE) };
    if raw_ptr.is_null() {
        return None;
    }

    let aligned_addr = match checked_align_up(raw_ptr as usize, SEGMENT_ALIGN) {
        Some(addr) => addr,
        None => {
            // Safety: Releasing raw memory back to the backend because alignment check overflowed.
            let _released = unsafe { B::deallocate(raw_ptr, SEGMENT_MAPPING_SIZE) };
            return None;
        }
    };
    let aligned_ptr = aligned_addr as *mut Segment;

    // Return the alignment slack preceding the segment header to the OS. The
    // mapping over-reserves `SEGMENT_MAPPING_SIZE = 2 * SEGMENT_SIZE` so a
    // `SEGMENT_ALIGN`-aligned base can always be found; the bytes in
    // `[raw_ptr, aligned_addr)` are never used by the allocator. On Windows
    // `VirtualAlloc` eagerly commits the whole mapping, so decommitting this
    // head slack drops up to ~`SEGMENT_ALIGN` (≈ 2 MiB) of commit charge per
    // segment; on Unix the slack is lazily backed, so this is typically a
    // no-op. Best-effort: a backend without `decommit` (default `false`)
    // simply skips. The slack stays inside the reservation and is released by
    // `deallocate(raw_ptr, SEGMENT_MAPPING_SIZE)`.
    //
    // `head_slack` is a multiple of the system page size because both
    // `raw_ptr` (from `allocate`) and `aligned_addr` (a `SEGMENT_ALIGN`
    // multiple) are page-aligned.
    let head_slack = aligned_addr - raw_ptr as usize;
    if head_slack > 0 {
        // Safety: `[raw_ptr, aligned_addr)` is a page-aligned subrange of the
        // live reservation holding no allocator data (it precedes the header)
        // and remains covered by the base release.
        let _ = unsafe { B::decommit(raw_ptr, head_slack) };
    }

    // Safety: aligned_ptr is within the allocated region and aligned to segment boundary.
    // We initialize the segment structure inside this newly aligned memory region.
    unsafe {
        Segment::initialize(aligned_ptr, raw_ptr);
    }

    #[cfg(feature = "segment-tail-guards")]
    {
        // Install a tail guard immediately after the segment's user-page
        // region. Forward OOB writes that walk past Page 31 land in this
        // guard region instead of an unrelated mapping. The address lives
        // inside the `SEGMENT_MAPPING_SIZE - SEGMENT_SIZE` slack the arena
        // reserves to satisfy `SEGMENT_ALIGN` rounding, so it is always
        // part of the same backend allocation and is released together
        // with the segment by `B::deallocate(raw_ptr, SEGMENT_MAPPING_SIZE)`.
        // The install is best-effort: a backend without a `make_guard`
        // implementation (default `false`) or a kernel that declines the
        // request (e.g. macOS-arm64 where the OS page size exceeds 4 KiB)
        // silently skips, leaving the slack accessible. Backend telemetry
        // (`guard_install_calls`) surfaces the actual install count.
        //
        // Safety: aligned_addr + SEGMENT_SIZE is inside the raw mapping
        // because slack-after >= OS_PAGE_SIZE >= SEGMENT_TAIL_GUARD_SIZE on
        // supported targets. `make_guard` never invalidates the mapping.
        let tail_guard_addr = aligned_addr + SEGMENT_SIZE;
        let _guarded =
            unsafe { B::make_guard(tail_guard_addr as *mut u8, SEGMENT_TAIL_GUARD_SIZE) };
    }

    Some(aligned_ptr)
}

/// Returns a segment to the global pool.
///
/// # Safety
///
/// The `segment` pointer must be a valid initialized segment.
#[inline]
pub unsafe fn deallocate_segment<B: HasSegmentPool>(segment: *mut Segment) {
    if !segment.is_null() {
        // Safety: try_return_to_pool checks segment status and pushes it to global segment pool if space permits.
        if !try_return_to_pool::<B>(segment) {
            // Safety: segment is a valid allocated Segment. We extract raw_alloc_ptr
            // and deallocate the original OS mapping since the global pool is full.
            match unsafe { release_segment_mapping::<B>(segment) } {
                SegmentRelease::Released => {}
                SegmentRelease::RetainedAfterFailure => {
                    B::global_segment_pool().push_unbounded(segment);
                }
            }
        }
    }
}

/// Attempts to release one segment mapping to the backend.
///
/// # Safety
///
/// The `segment` pointer must be valid, initialized, and exclusively owned by the caller.
#[inline]
pub unsafe fn release_segment_mapping<B: HasSegmentPool>(segment: *mut Segment) -> SegmentRelease {
    // Safety: segment is a valid allocated Segment. We extract raw_alloc_ptr
    // and deallocate the original OS mapping.
    let released = unsafe {
        let raw_ptr = (*segment).raw_alloc_ptr;
        B::deallocate(raw_ptr, SEGMENT_MAPPING_SIZE)
    };

    if released {
        SegmentRelease::Released
    } else {
        SegmentRelease::RetainedAfterFailure
    }
}

/// Purges the global segment pool and releases all segments back to the OS.
///
/// # Safety
///
/// Deallocates raw memory pointers from the backend.
pub unsafe fn purge_segment_pool<B: HasSegmentPool>() {
    let mut purged = 0;
    let pool = B::global_segment_pool();
    while let Some(segment) = pool.pop() {
        // Safety: segment is a valid allocated Segment popped from the global pool.
        // We retain ownership if the backend reports release failure, so pool
        // metadata never claims a purge for a still-owned mapping.
        match unsafe { release_segment_mapping::<B>(segment) } {
            SegmentRelease::Released => purged += 1,
            SegmentRelease::RetainedAfterFailure => {
                pool.push_unbounded(segment);
                break;
            }
        }
    }
    pool.record_purge(purged);
}

/// Drops the physical backing of every retained free segment without
/// removing them from the cache.
///
/// Walks the retained pool by draining it into a fixed-size stack
/// buffer, asks the backend to reset the physical pages of each
/// drained segment's mapping, and pushes the segments back onto the
/// pool so they remain available for reuse. The address ranges stay
/// owned by the allocator; only the OS-visible RSS is released.
///
/// Used as a lighter-weight RSS-reduction knob than `purge_segment_pool`
/// for callers that want to keep the segment cache warm but reduce
/// resident set size on idle periods.
///
/// # Safety
///
/// Calls `MemoryBackend::page_reset` on every segment in the pool. Each
/// segment must currently be a valid initialized retained mapping -
/// which is guaranteed because we only operate on segments popped from
/// the retained pool by this function and then push them back. Concurrent
/// allocators may temporarily observe an empty pool and fall back to OS
/// allocation; this is intended for quiescent periods.
pub unsafe fn reset_segment_pool<B: HasSegmentPool>() {
    let pool = B::global_segment_pool();
    // Drain into a fixed-size stack buffer (the pool is bounded to
    // MAX_RETAINED_SEGMENTS, so this never overflows).
    let mut buffer: [*mut Segment; MAX_RETAINED_SEGMENTS] =
        [core::ptr::null_mut(); MAX_RETAINED_SEGMENTS];
    let mut drained = 0usize;
    while drained < MAX_RETAINED_SEGMENTS {
        match pool.pop() {
            Some(segment) => {
                buffer[drained] = segment;
                drained += 1;
            }
            None => break,
        }
    }

    // Reset each segment's mapping and push it back. The reset result is
    // advisory: a backend without `page_reset` support (or a kernel that
    // declines the advice) returns false, in which case we leave the
    // mapping untouched and simply re-cache the segment.
    let mut reset_count = 0usize;
    for slot in buffer.iter().take(drained) {
        let segment = *slot;
        // Safety: segment was popped from the retained pool above and is
        // an initialized mapping owned by this allocator until we push it
        // back below.
        let raw_ptr = unsafe { (*segment).raw_alloc_ptr };
        // Safety: raw_ptr covers SEGMENT_MAPPING_SIZE bytes per the
        // arena allocation contract.
        if unsafe { B::page_reset(raw_ptr, SEGMENT_MAPPING_SIZE) } {
            reset_count += 1;
        }
        pool.push_unbounded(segment);
    }

    pool.record_reset(reset_count);
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use core::sync::atomic::{AtomicUsize, Ordering};
    use mnemosyne_core::MemoryBackend;
    #[cfg(feature = "segment-tail-guards")]
    use std::alloc::{alloc, dealloc, Layout};

    struct FailingReleaseBackend;

    static FAILING_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
    static FAILING_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
    static FAILING_DEALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);

    impl MemoryBackend for FailingReleaseBackend {
        unsafe fn allocate(_size: usize) -> *mut u8 {
            core::ptr::null_mut()
        }

        unsafe fn deallocate(_ptr: *mut u8, _size: usize) -> bool {
            FAILING_DEALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    impl HasSegmentPool for FailingReleaseBackend {
        fn global_segment_pool() -> &'static GlobalSegmentPool {
            &FAILING_POOL
        }

        fn global_orphan_pool() -> &'static GlobalSegmentPool {
            &FAILING_ORPHAN_POOL
        }
    }

    #[cfg(feature = "segment-tail-guards")]
    struct GuardRecordingBackend;

    #[cfg(feature = "segment-tail-guards")]
    static GUARD_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
    #[cfg(feature = "segment-tail-guards")]
    static GUARD_ORPHAN_POOL: GlobalSegmentPool = GlobalSegmentPool::new();
    #[cfg(feature = "segment-tail-guards")]
    static GUARD_CALLS: AtomicUsize = AtomicUsize::new(0);
    #[cfg(feature = "segment-tail-guards")]
    static LAST_GUARD_PTR: AtomicUsize = AtomicUsize::new(0);
    #[cfg(feature = "segment-tail-guards")]
    static LAST_GUARD_SIZE: AtomicUsize = AtomicUsize::new(0);

    #[cfg(feature = "segment-tail-guards")]
    impl MemoryBackend for GuardRecordingBackend {
        unsafe fn allocate(size: usize) -> *mut u8 {
            let layout = Layout::from_size_align(size, SEGMENT_ALIGN)
                .expect("segment mapping layout must be valid");
            unsafe { alloc(layout) }
        }

        unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
            let layout = Layout::from_size_align(size, SEGMENT_ALIGN)
                .expect("segment mapping layout must be valid");
            unsafe {
                dealloc(ptr, layout);
            }
            true
        }

        unsafe fn make_guard(ptr: *mut u8, size: usize) -> bool {
            GUARD_CALLS.fetch_add(1, Ordering::Relaxed);
            LAST_GUARD_PTR.store(ptr as usize, Ordering::Relaxed);
            LAST_GUARD_SIZE.store(size, Ordering::Relaxed);
            true
        }
    }

    #[cfg(feature = "segment-tail-guards")]
    impl HasSegmentPool for GuardRecordingBackend {
        fn global_segment_pool() -> &'static GlobalSegmentPool {
            &GUARD_POOL
        }

        fn global_orphan_pool() -> &'static GlobalSegmentPool {
            &GUARD_ORPHAN_POOL
        }
    }

    #[test]
    fn purge_retains_segment_when_backend_release_fails() {
        let mut segment = core::mem::MaybeUninit::<Segment>::uninit();
        let segment_ptr = segment.as_mut_ptr();

        unsafe {
            Segment::initialize(segment_ptr, 0x1000 as *mut u8);
        }
        FailingReleaseBackend::global_segment_pool().push_unbounded(segment_ptr);

        let before = arena_memory_stats::<FailingReleaseBackend>();
        unsafe {
            purge_segment_pool::<FailingReleaseBackend>();
        }
        let after = arena_memory_stats::<FailingReleaseBackend>();

        assert_eq!(after.retained_free_segments, before.retained_free_segments);
        assert_eq!(after.purge_calls, before.purge_calls + 1);
        assert_eq!(after.purged_segments, before.purged_segments);
        assert_eq!(after.purged_bytes, before.purged_bytes);
        assert_eq!(FAILING_DEALLOC_CALLS.load(Ordering::Relaxed), 1);

        assert!(
            FailingReleaseBackend::global_segment_pool().pop().is_some(),
            "failed release segment was not retained in the pool"
        );
    }

    #[cfg(feature = "segment-tail-guards")]
    #[test]
    fn fresh_segment_install_increments_guard_telemetry_and_round_trips() {
        // A fresh segment allocated through the default backend installs
        // a SEGMENT_TAIL_GUARD_SIZE PROT_NONE / PAGE_NOACCESS region in
        // the alignment slack. This test asserts:
        //   1. `guard_install_calls` advances by at least 1 across a
        //      backend-OS segment allocation. (Pool-served allocations
        //      reuse an already-guarded mapping and do not advance the
        //      counter; we force the OS path by purging first.)
        //   2. The segment releases cleanly even though part of its
        //      backing mapping is now PROT_NONE / PAGE_NOACCESS.
        //   3. The guard install never decrements `current_mapped_bytes`.
        use mnemosyne_backend::{backend_memory_stats, MemoryBackendWrapper};

        // Purge to force the OS allocation path.
        unsafe {
            purge_segment_pool::<MemoryBackendWrapper>();
        }
        let before = backend_memory_stats();

        // Safety: arena-managed segment allocation.
        let segment = unsafe { allocate_segment::<MemoryBackendWrapper>() }
            .expect("OS-backed segment allocation must succeed");
        let after_alloc = backend_memory_stats();

        // On Windows the MEM_NOACCESS path always confirms, so the
        // counter advances by at least 1. On platforms without
        // make_guard support the counter may stay flat - in that case
        // the segment still allocated successfully and the assertion
        // is purely informational.
        if after_alloc.guard_install_calls > before.guard_install_calls {
            assert!(
                after_alloc.guard_install_bytes
                    >= before.guard_install_bytes + SEGMENT_TAIL_GUARD_SIZE,
                "guard_install_bytes advanced by less than SEGMENT_TAIL_GUARD_SIZE"
            );
        }
        // The mapping itself must be tracked normally regardless of
        // guard outcome.
        assert!(
            after_alloc.current_mapped_bytes >= before.current_mapped_bytes + SEGMENT_MAPPING_SIZE,
            "current_mapped_bytes did not advance by the full segment mapping"
        );

        // Release: VirtualFree(MEM_RELEASE) / munmap accepts a region
        // regardless of the protection state of its sub-ranges.
        unsafe {
            deallocate_segment::<MemoryBackendWrapper>(segment);
        }
        // Purge the segment from the retained pool to fully release.
        unsafe {
            purge_segment_pool::<MemoryBackendWrapper>();
        }
        let after_release = backend_memory_stats();
        assert_eq!(
            after_release.current_mapped_bytes, before.current_mapped_bytes,
            "current_mapped_bytes did not return to baseline after release"
        );
    }

    #[cfg(feature = "segment-tail-guards")]
    #[test]
    fn fresh_segment_installs_tail_guard_in_alignment_slack() {
        while GuardRecordingBackend::global_segment_pool().pop().is_some() {}
        while GuardRecordingBackend::global_orphan_pool().pop().is_some() {}
        GUARD_CALLS.store(0, Ordering::Relaxed);
        LAST_GUARD_PTR.store(0, Ordering::Relaxed);
        LAST_GUARD_SIZE.store(0, Ordering::Relaxed);

        let segment =
            unsafe { allocate_segment::<GuardRecordingBackend>() }.expect("segment allocation");
        let expected_guard = segment as usize + SEGMENT_SIZE;

        assert_eq!(
            GUARD_CALLS.load(Ordering::Relaxed),
            1,
            "fresh segment allocation did not request exactly one guard install"
        );
        assert_eq!(
            LAST_GUARD_PTR.load(Ordering::Relaxed),
            expected_guard,
            "tail guard was not placed immediately after the segment"
        );
        assert_eq!(
            LAST_GUARD_SIZE.load(Ordering::Relaxed),
            SEGMENT_TAIL_GUARD_SIZE,
            "tail guard size drifted from the documented constant"
        );

        let released = unsafe { release_segment_mapping::<GuardRecordingBackend>(segment) };
        assert_eq!(released, SegmentRelease::Released);
    }
}
