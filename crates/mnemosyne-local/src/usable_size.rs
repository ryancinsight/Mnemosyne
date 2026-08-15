use crate::{LocalAllocatorSelector, ThreadAllocator, ThreadAllocatorStats};
use mnemosyne_arena::HasSegmentPool;
use mnemosyne_core::policy::AllocPolicy;
use mnemosyne_core::types::{Segment, locate_segment};

/// Returns the actual usable byte count of the allocation at `ptr`.
///
/// For small allocations this returns the size-class block size (which
/// may exceed the original allocation request because Mnemosyne rounds
/// up to the next size class). For large/huge allocations it returns
/// the distance from `ptr` to the end of the recorded payload mapping.
/// Returns `0` for a null pointer.
///
/// Mirrors `mi_usable_size` (mimalloc) and `malloc_usable_size`
/// (glibc/jemalloc): the value is the maximum number of bytes the
/// caller may dereference through `ptr` without overflowing the
/// allocation. Useful for Rust `Vec<T>` capacity-rounding and for any
/// caller that wants to know the allocator's actual reservation
/// without doing a follow-up `realloc`.
///
/// # Safety
///
/// `ptr` must either be null or be a pointer previously returned by a
/// Mnemosyne allocation entry point. Calling this with a pointer that
/// originated from a different allocator is undefined behavior; the
/// function uses the same segment-rounding classification as
/// `thread_free` and dereferences the resulting segment header.
/// # Examples
///
/// ```
/// use mnemosyne_local::{thread_alloc, thread_free, usable_size};
/// use mnemosyne_core::StandardPolicy;
/// use mnemosyne_backend::MemoryBackendWrapper as Backend;
///
/// // SAFETY: `p` is a live block from this allocator, which is exactly what
/// // `usable_size` requires; passing a foreign pointer is undefined behaviour.
/// unsafe {
///     let p = thread_alloc::<StandardPolicy, Backend>(100, 8);
///     assert!(!p.is_null());
///
///     // The answer is what the caller may dereference, not what it asked
///     // for: requests round up to a size class, so this is >= 100 and the
///     // whole reported span is writable.
///     let usable = usable_size(p);
///     assert!(usable >= 100);
///     p.write_bytes(0xFF, usable);
///
///     thread_free::<StandardPolicy, Backend>(p);
/// }
///
/// // Null reports zero rather than faulting.
/// // SAFETY: null is explicitly accepted by the contract.
/// assert_eq!(unsafe { usable_size(core::ptr::null_mut()) }, 0);
/// ```
#[inline(always)]
pub unsafe fn usable_size(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }

    // SAFETY: `ptr` is a non-null allocator-owned pointer per the `# Safety`
    // contract, satisfying `locate_segment`'s precondition.
    let (segment, page_index) = unsafe { locate_segment(ptr) };

    // `page_index == 0` must be tested *before* any page-metadata read, not
    // after. Page 0 is segment metadata and is never allocated from, so a zero
    // index means the user pointer is `SEGMENT_ALIGN`-aligned — which for a
    // huge allocation happens when its alignment request pushes the payload
    // onto a segment boundary past the header. `locate_segment` then masks down
    // to that boundary, and the "segment" it names is payload, not a header.
    // Reading `pages[0].block_size` there interprets the caller's own bytes as
    // page metadata: uninitialized memory in the abstract machine, and a
    // live-data misread in practice, since a non-zero byte would classify a
    // huge allocation as small and report a garbage size.
    if page_index > 0 {
        // SAFETY: `page_index` is in [1, PAGES_PER_SEGMENT), so `segment` is
        // the real header of the segment containing `ptr` and its page metadata
        // was initialized by `Segment::initialize`.
        let page = unsafe { (*segment).pages.get_unchecked(page_index) };
        let size = page.block_size;
        if size > 0 {
            return size;
        }
    }

    // Large/huge allocation: recover the size from the metadata-slot segment.
    // SAFETY: `page.block_size == 0` (or `page_index == 0`) identifies a
    // large/huge allocation, which stores its segment pointer in the metadata
    // slot immediately preceding the user pointer — exactly
    // `huge_allocation_size`'s precondition.
    unsafe { huge_allocation_size(ptr) }
}

/// Returns the usable byte size of a large/huge allocation from its metadata
/// slot: the recorded `pages[0].alloc_count` when set, else the mapping suffix
/// from `ptr`.
///
/// This is the single authoritative huge-allocation size recovery, shared by
/// [`usable_size`] and the free-profiling path so the metadata-slot layout and
/// its fallback live in one place.
///
/// # Safety
///
/// `ptr` must be a non-null user pointer from a Mnemosyne *large/huge*
/// allocation, so the pointer slot immediately preceding it holds a valid
/// segment header (`(ptr as *mut *mut Segment).sub(1)`).
#[inline]
pub(crate) unsafe fn huge_allocation_size(ptr: *mut u8) -> usize {
    // SAFETY: per the contract, the slot one pointer before `ptr` holds the
    // originating segment header written at `allocate_large_or_huge` time.
    let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
    let size = unsafe { (*segment).pages[0].alloc_count };
    if size > 0 {
        size
    } else {
        // SAFETY: a zero recorded size means a non-segment-aligned huge mapping;
        // `huge_mapping_suffix_from` returns the distance to the mapping end.
        unsafe { (*segment).huge_mapping_suffix_from(ptr) }
    }
}

/// Returns a statistics snapshot for the current thread's allocator under
/// policy `P`.
///
/// `P` selects which allocator is reported, and is not decoration. ADR 0001
/// gives every `(backend, encryption mode)` pair its own `ThreadAllocator`
/// cache, so a process allocating through `HardenedPolicy` and a process
/// allocating through `StandardPolicy` have separate counters. Passing the
/// policy the caller actually allocates with is what makes the snapshot
/// describe their allocator rather than a neighbouring one (ADR 0008).
/// # Examples
///
/// ```
/// use mnemosyne_local::{thread_alloc, thread_allocator_stats, thread_free};
/// use mnemosyne_core::{StandardPolicy, policy::HardenedPolicy};
/// use mnemosyne_backend::MemoryBackendWrapper as Backend;
///
/// let before = thread_allocator_stats::<StandardPolicy, Backend>();
///
/// // SAFETY: allocated and freed once, under the policy named below.
/// let p = unsafe { thread_alloc::<StandardPolicy, Backend>(48, 8) };
/// assert!(!p.is_null());
///
/// let during = thread_allocator_stats::<StandardPolicy, Backend>();
/// assert_eq!(
///     during.current_thread_live_allocations,
///     before.current_thread_live_allocations + 1,
/// );
///
/// // `P` selects which allocator is reported. Each (backend, encryption mode)
/// // pair owns a separate cache, so asking under a policy you did not
/// // allocate with describes a different allocator — not this one.
/// let other = thread_allocator_stats::<HardenedPolicy, Backend>();
/// assert_eq!(other.current_thread_live_allocations, 0);
///
/// // SAFETY: `p` came from the allocation above and is freed exactly once.
/// unsafe { thread_free::<StandardPolicy, Backend>(p) };
/// ```
pub fn thread_allocator_stats<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>()
-> ThreadAllocatorStats {
    B::with_allocator_for_policy::<P, _>(|alloc| alloc.stats()).unwrap_or_else(|| {
        ThreadAllocatorStats {
            cross_thread_reclaimed_blocks: ThreadAllocator::<B>::cross_thread_reclaimed_blocks(),
            ..ThreadAllocatorStats::default()
        }
    })
}
