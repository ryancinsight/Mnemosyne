//! Thread-local cache allocation and deallocation routing.

#![no_std]

extern crate std;

pub mod local_alloc;

pub use local_alloc::{SizeClassOccupancy, ThreadAllocator, ThreadAllocatorStats};

use core::ptr::NonNull;
use mnemosyne_arena::{allocate_large_or_huge, deallocate_large_or_huge, HasSegmentPool};
use mnemosyne_core::constants::{PAGES_PER_SEGMENT, PAGE_SIZE, SEGMENT_SIZE};
use mnemosyne_core::types::{Block, Page, Segment};
use mnemosyne_core::validation::{is_valid_alloc_request, is_valid_layout_alloc_request};

use mnemosyne_core::policy::AllocPolicy;

/// Applies allocation-time initialization required by `P`.
///
/// # Safety
///
/// `ptr` must be valid for writes of `size` bytes and must refer to memory
/// owned by the current allocation operation.
#[inline(always)]
unsafe fn initialize_allocated_bytes<P: AllocPolicy>(ptr: *mut u8, size: usize) {
    if P::ZERO_INITIALIZE {
        // Safety: the caller guarantees ptr is valid and writable for size bytes.
        unsafe {
            core::ptr::write_bytes(ptr, 0, size);
        }
    } else if P::ENABLE_POISONING {
        // Safety: the caller guarantees ptr is valid and writable for size bytes.
        unsafe {
            core::ptr::write_bytes(ptr, P::POISON_ALLOC_BYTE, size);
        }
    }
}

/// Applies free-time poisoning required by `P`.
///
/// # Safety
///
/// `ptr` must be valid for writes of `size` bytes until the surrounding free
/// operation completes.
#[inline(always)]
unsafe fn poison_freed_bytes<P: AllocPolicy>(ptr: *mut u8, size: usize) {
    if P::ENABLE_POISONING {
        // Safety: the caller guarantees ptr is valid and writable for size bytes.
        unsafe {
            core::ptr::write_bytes(ptr, P::POISON_FREE_BYTE, size);
        }
    }
}

/// Trait resolving dynamic backend-specific thread-local cache selection.
pub trait LocalAllocatorSelector<B: HasSegmentPool>: HasSegmentPool {
    /// Evaluates the closure with a mutable reference to the thread-local allocator cache.
    ///
    /// Returns `None` if the allocator is already borrowed (re-entrancy detected).
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R>;

    /// Runs `f` with the thread-local allocator when the allocation guard is clear.
    ///
    /// Returns `None` when allocation is already in progress on this thread.
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R>;

    /// Returns the raw pointer to the thread-local allocator cache.
    fn get_allocator_ptr() -> *mut core::ffi::c_void;
}

/// Helper macro to generate zero-cost backend-specific thread-local cache pools.
#[macro_export]
macro_rules! impl_local_allocator_selector {
    ($backend:ty) => {
        const _: () = {
            std::thread_local! {
                static IS_ALLOCATING: std::cell::Cell<bool> = std::cell::Cell::new(false);
                static ALLOCATOR: std::cell::UnsafeCell<$crate::ThreadAllocator<$backend>> = std::cell::UnsafeCell::new($crate::ThreadAllocator::new());
            }

            impl $crate::LocalAllocatorSelector<$backend> for $backend {
                #[inline(always)]
                fn with_allocator<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    IS_ALLOCATING.with(|c| {
                        if c.get() {
                            return None;
                        }
                        c.set(true);
                        // Safety: ALLOCATOR is thread-local, so no other thread can
                        // access this cell. IS_ALLOCATING rejects nested access on the
                        // same thread before a second mutable reference can be created.
                        let result = ALLOCATOR.with(|cell| unsafe { f(&mut *cell.get()) });
                        c.set(false);
                        Some(result)
                    })
                }

                #[inline(always)]
                fn with_allocator_guard<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    IS_ALLOCATING.with(|c| {
                        if c.get() {
                            return None;
                        }
                        c.set(true);
                        // Safety: ALLOCATOR is thread-local, so no other thread can
                        // access this cell. IS_ALLOCATING rejects nested access on the
                        // same thread before a second mutable reference can be created.
                        let result = ALLOCATOR.with(|cell| unsafe { f(&mut *cell.get()) });
                        c.set(false);
                        Some(result)
                    })
                }

                #[inline(always)]
                fn get_allocator_ptr() -> *mut core::ffi::c_void {
                    ALLOCATOR.with(|cell| cell.get().cast())
                }
            }
        };
    };
}

impl_local_allocator_selector!(mnemosyne_backend::MemoryBackendWrapper);
impl_local_allocator_selector!(mnemosyne_backend::CudaUnifiedBackend);

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
pub unsafe fn usable_size(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }

    if (ptr as usize) & (SEGMENT_SIZE - 1) == 0 {
        // Safety: large/huge allocations store the segment pointer in
        // the metadata slot immediately preceding the user pointer.
        let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        let huge_size = unsafe { (*segment).pages[0].block_size };
        // huge_size is the full backend mapping size; the usable
        // payload is the suffix from the user pointer to the end of
        // the mapping.
        return (segment as usize + huge_size) - ptr as usize;
    }

    let segment_addr = (ptr as usize) & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr as usize - segment_addr) / PAGE_SIZE;

    debug_assert!(
        page_index < PAGES_PER_SEGMENT,
        "usable_size pointer page_index {} exceeds segment page count {}",
        page_index,
        PAGES_PER_SEGMENT
    );

    // Safety: for small allocations, page_index is in [1, PAGES_PER_SEGMENT)
    // and the target page records the size-class block size. For non-segment
    // aligned huge allocations, the user pointer still lands within the
    // segment metadata window but the target page's block_size remains zero,
    // routing below to the metadata-slot fallback.
    let page = unsafe { &(*segment).pages[page_index] };
    if page.block_size > 0 {
        return page.block_size;
    }

    // Safety: non-segment-aligned large/huge allocations store the segment
    // pointer in the metadata slot immediately preceding the user pointer.
    let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
    let huge_size = unsafe { (*segment).pages[0].block_size };
    (segment as usize + huge_size) - ptr as usize
}

/// Returns a statistics snapshot for the current thread allocator.
pub fn thread_allocator_stats<B: HasSegmentPool + LocalAllocatorSelector<B>>(
) -> ThreadAllocatorStats {
    B::with_allocator(|alloc| alloc.stats()).unwrap_or_else(|| ThreadAllocatorStats {
        cross_thread_reclaimed_blocks: ThreadAllocator::<B>::cross_thread_reclaimed_blocks(),
        ..ThreadAllocatorStats::default()
    })
}

/// Allocates a memory block of the given size and alignment.
///
/// # Safety
///
/// This function is unsafe because it handles raw pointers and manual layouts.
pub unsafe fn thread_alloc<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    size: usize,
    align: usize,
) -> *mut u8 {
    if !is_valid_alloc_request(size, align) {
        return core::ptr::null_mut();
    }

    unsafe { thread_alloc_checked::<P, B>(size, align) }
}

/// Allocates from a Rust `Layout`-validated request.
///
/// This preserves the global allocator hot path by relying on `Layout` for the
/// nonzero power-of-two alignment contract while still enforcing Mnemosyne's
/// allocator-specific bounds.
///
/// # Safety
///
/// `size` must be nonzero and `align` must come from a valid `Layout`.
#[inline(always)]
pub unsafe fn thread_alloc_layout<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    size: usize,
    align: usize,
) -> *mut u8 {
    if !is_valid_layout_alloc_request(size, align) {
        return core::ptr::null_mut();
    }

    debug_assert!(
        align != 0 && align.is_power_of_two(),
        "Layout-validated allocation received invalid alignment {align}"
    );
    unsafe { thread_alloc_checked::<P, B>(size, align) }
}

#[inline(always)]
unsafe fn thread_alloc_checked<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    size: usize,
    align: usize,
) -> *mut u8 {
    if align > 16 {
        // Fall back to direct arena/huge allocation to break recursion and get high alignment (64KB aligned page-starts).
        // Safety: allocate_large_or_huge handles safety.
        let ptr = unsafe { allocate_large_or_huge::<B>(size, align) };
        if !ptr.is_null() {
            unsafe { initialize_allocated_bytes::<P>(ptr, size) };
        }
        return ptr;
    }

    let adjusted_size = if size < align { align } else { size };

    let Some(ptr) = B::with_allocator_guard(|alloc| {
        // Safety: alloc is exclusively borrowed and valid. alloc.alloc handles safety.
        unsafe { alloc.alloc(adjusted_size) }
    }) else {
        // Fall back to direct arena/huge allocation to break recursion.
        // Safety: size and align are forwarded. allocate_large_or_huge handles safety.
        let ptr = unsafe { allocate_large_or_huge::<B>(adjusted_size, align) };
        if !ptr.is_null() {
            unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
        }
        return ptr;
    };

    let final_ptr = if ptr.is_null() {
        // Fallback for large allocations or failed cache allocations.
        // Safety: Fallback route using backend B.
        unsafe { allocate_large_or_huge::<B>(adjusted_size, align) }
    } else {
        ptr
    };

    if !final_ptr.is_null() {
        unsafe { initialize_allocated_bytes::<P>(final_ptr, adjusted_size) };
    }

    final_ptr
}

/// Frees a memory block.
///
/// # Safety
///
/// The ptr must be valid and must have been returned by a previous allocation.
#[inline(always)]
pub unsafe fn thread_free<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
) {
    if ptr.is_null() {
        return;
    }

    if (ptr as usize) & (SEGMENT_SIZE - 1) == 0 {
        if P::ENABLE_POISONING {
            // Safety: ptr is a valid large/huge allocation, so the segment header pointer
            // is stored in the metadata slot immediately preceding the user pointer.
            let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            let huge_size = unsafe { (*segment).pages[0].block_size };
            let size = (segment as usize + huge_size) - ptr as usize;
            unsafe { poison_freed_bytes::<P>(ptr, size) };
        }
        // Safety: ptr was returned by a large/huge allocation, so the segment header pointer
        // is stored in the metadata slot immediately preceding the user pointer.
        let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        let _released = unsafe { deallocate_large_or_huge::<B>(ptr, segment) };
        return;
    }

    let segment_addr = (ptr as usize) & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;

    // Small-allocation classification invariants.
    //
    // The unsafe caller contract guarantees `ptr` was returned by a previous
    // small allocation, which means:
    //   1. `page_index` lies in `[1, PAGES_PER_SEGMENT)` because Page 0 is
    //      the segment header and small block-starts begin at Page 1.
    //   2. `page.block_size > 0` because every page used for small frees was
    //      initialized via `Page::initialize_free_list`.
    //   3. The offset of `ptr` inside its page is a multiple of
    //      `page.block_size` because the allocator hands out block-aligned
    //      slot starts.
    // The `debug_assert!` checks pin these invariants in debug builds while
    // the release path remains branch-free for verified caller inputs.
    let page_index = (ptr as usize - segment_addr) / PAGE_SIZE;
    debug_assert!(
        page_index > 0,
        "small allocations never live in metadata page 0"
    );
    debug_assert!(
        page_index < PAGES_PER_SEGMENT,
        "small free ptr must fall within the segment's page array"
    );
    // Safety: page_index is within bounds (1..PAGES_PER_SEGMENT) since ptr is a small alloc.
    let page = unsafe { &mut (*segment).pages[page_index] };
    if page.block_size == 0 {
        if P::ENABLE_POISONING {
            // Safety: ptr is a valid large/huge allocation, so the segment header pointer
            // is stored in the metadata slot immediately preceding the user pointer.
            let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            let huge_size = unsafe { (*segment).pages[0].block_size };
            let size = (segment as usize + huge_size) - ptr as usize;
            unsafe { poison_freed_bytes::<P>(ptr, size) };
        }
        // Safety: non-small allocations keep the owning segment in the
        // metadata slot immediately preceding the user pointer.
        let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        let _released = unsafe { deallocate_large_or_huge::<B>(ptr, segment) };
        return;
    }
    debug_assert_eq!(
        (ptr as usize - (segment_addr + page_index * PAGE_SIZE)) % page.block_size,
        0,
        "small free ptr must be aligned to the page's block stride"
    );

    if P::ENABLE_POISONING {
        unsafe { poison_freed_bytes::<P>(ptr, page.block_size) };
    }

    let block = ptr as *mut Block;

    // Determine whether the current thread holds the segment owner token.
    // Safety: segment header is initialized and owner field is valid.
    let owner = unsafe { (*segment).owner };
    let current_allocator = B::get_allocator_ptr();

    if owner.matches(current_allocator) {
        debug_assert!(page.alloc_count > 0, "local free observed zero alloc_count");
        let was_full = page.alloc_count == page.max_blocks;
        let becomes_empty = page.alloc_count == 1;
        // Safety: the owner token matched this thread's allocator. If the page
        // is not full and the free cannot trigger non-current segment reclaim,
        // the page-local metadata update does not need access to allocator lists.
        if !was_full && (!becomes_empty || unsafe { (*segment).is_current }) {
            unsafe {
                (*block).next = page.free;
                page.free = Some(NonNull::new_unchecked(block));
                page.alloc_count -= 1;
            }
            return;
        }

        let res = B::with_allocator(|alloc| {
            // Safety: segment and block are verified to be owned by this thread allocator.
            // Local free: try to update the page metadata in the thread cache.
            unsafe {
                (*block).next = page.free;
                page.free = Some(NonNull::new_unchecked(block));
                let was_full = page.alloc_count == page.max_blocks;
                if page.alloc_count > 0 {
                    page.alloc_count -= 1;
                }
                if was_full {
                    if let Some(class) = mnemosyne_core::size_to_class(page.block_size) {
                        if alloc.unlink_full_page(page as *mut Page, class) {
                            page.next_page = alloc.active_pages[class];
                            alloc.active_pages[class] =
                                Some(NonNull::new_unchecked(page as *mut Page));
                        }
                    }
                }
                if page.alloc_count == 0 && !alloc.is_current_segment(segment) {
                    alloc.try_reclaim_segment(segment);
                }
            }
        });

        if res.is_none() {
            // Re-entrant/borrowed local free: push onto the owning page's queue.
            // The owner batch-reclaims this block after local page lists are exhausted.
            unsafe {
                page.thread_free.push(NonNull::new_unchecked(block));
            }
        }
        return;
    }

    // Cross-thread or orphan free: push onto the owning page's queue. The owner
    // batch-reclaims this block after local page lists are exhausted.
    unsafe {
        page.thread_free.push(NonNull::new_unchecked(block));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mnemosyne_backend::MemoryBackendWrapper;
    use mnemosyne_core::constants::MAX_ALLOC_SIZE;
    use mnemosyne_core::policy::StandardPolicy;

    #[test]
    fn usable_size_returns_block_size_for_small_allocations() {
        // Mnemosyne rounds small allocation requests up to the next
        // size class, so the usable size should match `class_to_size`
        // for every (request, alignment) pair the small-alloc test
        // sweep exercises, regardless of the *requested* size.
        for &(req_size, req_align) in &[(8usize, 8usize), (16, 8), (32, 16), (64, 8), (1024, 8)] {
            let ptr = unsafe {
                thread_alloc::<StandardPolicy, MemoryBackendWrapper>(req_size, req_align)
            };
            assert!(
                !ptr.is_null(),
                "alloc({req_size}, {req_align}) returned null"
            );

            let reported = unsafe { usable_size(ptr) };
            assert!(
                reported >= req_size,
                "usable_size({req_size}, {req_align}) = {reported} is below the request"
            );
            assert!(
                reported >= req_align,
                "usable_size({req_size}, {req_align}) = {reported} is below the adjusted minimum (alignment)"
            );
            // The reported size is whatever size class the page is
            // sliced into; verify it matches a real class.
            let segment_addr = (ptr as usize) & !(SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;
            let page_index = (ptr as usize - segment_addr) / PAGE_SIZE;
            let page = unsafe { &(*segment).pages[page_index] };
            assert_eq!(
                reported, page.block_size,
                "usable_size disagrees with the page's recorded block_size"
            );

            unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
        }
    }

    #[test]
    fn usable_size_returns_payload_remainder_for_huge_allocations() {
        // Direct large allocation through the arena. The returned
        // pointer carries enough payload to cover the requested size,
        // and `usable_size` reports at least that much (it may report
        // more because the arena reserves alignment slack).
        let request = 4 * 1024 * 1024;
        for &align in &[8usize, 64 * 1024, 1024 * 1024, SEGMENT_SIZE] {
            // Safety: power-of-two alignment, non-zero size.
            let ptr = unsafe {
                mnemosyne_arena::allocate_large_or_huge::<MemoryBackendWrapper>(request, align)
            };
            assert!(!ptr.is_null(), "huge allocation failed for align {align}");

            let reported = unsafe { usable_size(ptr) };
            assert!(
                reported >= request,
                "usable_size = {reported} is below the requested huge size {request} for align {align}"
            );

            let recovered = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            let _released = unsafe {
                mnemosyne_arena::deallocate_large_or_huge::<MemoryBackendWrapper>(ptr, recovered)
            };
        }
    }

    #[test]
    fn usable_size_returns_zero_for_null_pointer() {
        let reported = unsafe { usable_size(core::ptr::null_mut()) };
        assert_eq!(reported, 0);
    }

    #[test]
    fn small_alloc_returns_block_aligned_ptr_outside_metadata_page() {
        // The small-free classifier in `thread_free` relies on three
        // invariants: `page_index >= 1`, `page_index < PAGES_PER_SEGMENT`,
        // and `(ptr - page_start) % page.block_size == 0`. Verify each one
        // against the live allocation grid that customers actually observe.
        for &(req_size, req_align) in &[(8usize, 8usize), (16, 8), (32, 16), (64, 8), (1024, 8)] {
            let ptr = unsafe {
                thread_alloc::<StandardPolicy, MemoryBackendWrapper>(req_size, req_align)
            };
            assert!(
                !ptr.is_null(),
                "alloc({req_size}, {req_align}) returned null"
            );

            let segment_addr = (ptr as usize) & !(SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;
            let page_index = (ptr as usize - segment_addr) / PAGE_SIZE;

            assert!(
                page_index >= 1,
                "alloc({req_size}, {req_align}) ptr {ptr:?} landed in metadata Page 0"
            );
            assert!(
                page_index < PAGES_PER_SEGMENT,
                "alloc({req_size}, {req_align}) page_index {page_index} >= PAGES_PER_SEGMENT"
            );
            let page = unsafe { &(*segment).pages[page_index] };
            assert!(
                page.block_size > 0,
                "alloc({req_size}, {req_align}) targeted an uninitialized page"
            );
            let offset = (ptr as usize) - (segment_addr + page_index * PAGE_SIZE);
            assert_eq!(
                offset % page.block_size,
                0,
                "alloc({req_size}, {req_align}) ptr is not aligned to block stride {} of its size class",
                page.block_size,
            );

            unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
        }
    }

    #[test]
    fn reentrant_current_segment_local_free_uses_metadata_fast_path() {
        let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(32, 8) };
        assert!(
            !ptr.is_null(),
            "reentrant local-free setup allocation failed"
        );

        let segment_addr = (ptr as usize) & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr as usize - segment_addr) / PAGE_SIZE;
        let page = unsafe { &mut (*segment).pages[page_index] };

        assert_eq!(page.alloc_count, 1);
        assert!(
            page.thread_free.is_empty(),
            "thread_free list should start empty before reentrant free"
        );

        MemoryBackendWrapper::with_allocator(|_| {
            unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
        });

        assert_eq!(page.alloc_count, 0);
        assert!(
            page.thread_free.is_empty(),
            "current-segment local free should not enqueue into page-local thread_free"
        );
        assert_eq!(page.free.map(NonNull::as_ptr), Some(ptr as *mut Block));
    }

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
}
