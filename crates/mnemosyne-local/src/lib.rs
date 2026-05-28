//! Thread-local cache allocation and deallocation routing.

#![no_std]

extern crate std;

pub mod local_alloc;

pub use local_alloc::{SizeClassOccupancy, ThreadAllocator, ThreadAllocatorStats};

use core::ptr::NonNull;
use mnemosyne_arena::{allocate_large_or_huge, deallocate_large_or_huge, HasSegmentPool};
use mnemosyne_core::constants::{MAX_ALLOC_SIZE, PAGES_PER_SEGMENT, PAGE_SIZE, SEGMENT_SIZE};
use mnemosyne_core::types::{Block, Page, Segment};

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
    /// Evaluates the closure with a reference to the thread-local allocator cache.
    fn with_allocator<R>(f: impl FnOnce(&core::cell::RefCell<ThreadAllocator<B>>) -> R) -> R;

    /// Checks if we are currently allocating; if not, sets the flag to true and returns false.
    /// Otherwise returns true (re-entrancy detected).
    fn check_and_set_allocating() -> bool;

    /// Sets the re-entrancy / TLS allocation state flag.
    fn set_is_allocating(val: bool);
}

/// Helper macro to generate zero-cost backend-specific thread-local cache pools.
#[macro_export]
macro_rules! impl_local_allocator_selector {
    ($backend:ty) => {
        const _: () = {
            std::thread_local! {
                static IS_ALLOCATING: std::cell::Cell<bool> = std::cell::Cell::new(false);
                static ALLOCATOR: std::cell::RefCell<$crate::ThreadAllocator<$backend>> = std::cell::RefCell::new($crate::ThreadAllocator::new());
            }

            impl $crate::LocalAllocatorSelector<$backend> for $backend {
                #[inline(always)]
                fn with_allocator<R>(
                    f: impl FnOnce(&std::cell::RefCell<$crate::ThreadAllocator<$backend>>) -> R,
                ) -> R {
                    ALLOCATOR.with(f)
                }

                #[inline(always)]
                fn check_and_set_allocating() -> bool {
                    IS_ALLOCATING.with(|c| {
                        if c.get() {
                            true
                        } else {
                            c.set(true);
                            false
                        }
                    })
                }

                #[inline(always)]
                fn set_is_allocating(val: bool) {
                    IS_ALLOCATING.with(|c| c.set(val))
                }
            }
        };
    };
}

impl_local_allocator_selector!(mnemosyne_backend::MemoryBackendWrapper);
impl_local_allocator_selector!(mnemosyne_backend::CudaUnifiedBackend);

/// Returns a statistics snapshot for the current thread allocator.
pub fn thread_allocator_stats<B: HasSegmentPool + LocalAllocatorSelector<B>>(
) -> ThreadAllocatorStats {
    B::with_allocator(|alloc_ref| {
        if let Ok(alloc) = alloc_ref.try_borrow() {
            alloc.stats()
        } else {
            ThreadAllocatorStats {
                cross_thread_reclaimed_blocks: ThreadAllocator::<B>::cross_thread_reclaimed_blocks(
                ),
                ..ThreadAllocatorStats::default()
            }
        }
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
    if size == 0 {
        return core::ptr::null_mut();
    }
    if size > MAX_ALLOC_SIZE {
        return core::ptr::null_mut();
    }
    if align == 0 || !align.is_power_of_two() || align > SEGMENT_SIZE {
        return core::ptr::null_mut();
    }

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

    // Check if we are re-entering (e.g. during TLS initialization or recursive calls)
    let bypassed = B::check_and_set_allocating();

    if bypassed {
        // Fall back to direct arena/huge allocation to break recursion.
        // Safety: size and align are forwarded. allocate_large_or_huge handles safety.
        let ptr = unsafe { allocate_large_or_huge::<B>(adjusted_size, align) };
        if !ptr.is_null() {
            unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
        }
        return ptr;
    }

    // Attempt allocation through the thread-local cache.
    let ptr = B::with_allocator(|alloc_ref| {
        if let Ok(mut alloc) = alloc_ref.try_borrow_mut() {
            // Safety: alloc is exclusively borrowed and valid. alloc.alloc handles safety.
            unsafe { alloc.alloc(adjusted_size) }
        } else {
            core::ptr::null_mut()
        }
    });

    B::set_is_allocating(false);

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
pub unsafe fn thread_free<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
) {
    if ptr.is_null() {
        return;
    }

    // Determine if ptr is a large/huge allocation.
    //
    // Safety invariant: `allocate_large_or_huge` rejects alignments above
    // `SEGMENT_SIZE`, so every non-segment-aligned large/huge pointer still
    // rounds down to the initialized segment header. Segment-aligned huge
    // pointers use the metadata slot directly and avoid a header-rounding
    // dereference.
    let is_large_or_huge = if (ptr as usize) & (SEGMENT_SIZE - 1) == 0 {
        true
    } else {
        let segment_addr = (ptr as usize) & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        // Safety: for small allocations and large/huge allocations with alignment < SEGMENT_SIZE,
        // segment_addr is the correct segment header and is safe to dereference.
        unsafe { (*segment).pages[0].block_size > 0 }
    };

    if P::ENABLE_POISONING {
        let size = if is_large_or_huge {
            // Safety: ptr is a valid large/huge allocation, so the segment header pointer
            // is stored in the metadata slot immediately preceding the user pointer.
            let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            let huge_size = unsafe { (*segment).pages[0].block_size };
            (segment as usize + huge_size) - ptr as usize
        } else {
            let segment_addr = (ptr as usize) & !(SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;
            let page_index = (ptr as usize - segment_addr) / PAGE_SIZE;
            // Safety: page_index is within bounds of segment pages array.
            let page = unsafe { &mut (*segment).pages[page_index] };
            page.block_size
        };
        unsafe { poison_freed_bytes::<P>(ptr, size) };
    }

    if is_large_or_huge {
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
    debug_assert!(
        page.block_size > 0,
        "small free ptr must target an initialized page"
    );
    debug_assert_eq!(
        (ptr as usize - (segment_addr + page_index * PAGE_SIZE)) % page.block_size,
        0,
        "small free ptr must be aligned to the page's block stride"
    );

    let block = ptr as *mut Block;

    // Determine whether the current thread holds the segment owner token.
    // Safety: segment header is initialized and owner field is valid.
    let owner = unsafe { (*segment).owner };

    let current_alloc_ptr = B::with_allocator(|cell| cell.as_ptr());

    if owner.matches(current_alloc_ptr) {
        // Local free: try to update the page metadata in the thread cache.
        let success = B::with_allocator(|alloc_ref| {
            if let Ok(mut alloc) = alloc_ref.try_borrow_mut() {
                // Safety: segment and block are verified to be owned by this thread allocator.
                // We thread block into page free list and try reclaiming the segment if empty.
                unsafe {
                    (*block).next = page.free;
                    page.free = Some(NonNull::new_unchecked(block));
                    let was_full = page.alloc_count == page.max_blocks;
                    if page.alloc_count > 0 {
                        page.alloc_count -= 1;
                    }
                    if was_full {
                        if let Some(class) = mnemosyne_core::size_to_class(page.block_size) {
                            alloc.unlink_page(page as *mut Page, class);
                            page.next_page = alloc.active_pages[class];
                            alloc.active_pages[class] =
                                Some(NonNull::new_unchecked(page as *mut Page));
                        }
                    }
                    if page.alloc_count == 0 && !alloc.is_current_segment(segment) {
                        alloc.try_reclaim_segment(segment);
                    }
                }
                true
            } else {
                false
            }
        });

        if !success {
            // Re-entrant/borrowed local free: use the page-local atomic queue.
            // The owner will batch-reclaim this block after local page lists are exhausted.
            unsafe {
                page.thread_free.push(NonNull::new_unchecked(block));
            }
        }
    } else {
        // Cross-thread or orphan free: push onto the owning page's thread_free queue.
        // Safety: block is valid and page thread_free list is atomic.
        unsafe {
            page.thread_free.push(NonNull::new_unchecked(block));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mnemosyne_backend::MemoryBackendWrapper;
    use mnemosyne_core::policy::StandardPolicy;

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
    fn reentrant_local_free_uses_page_queue() {
        let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(32, 8) };
        assert!(!ptr.is_null());

        let segment_addr = (ptr as usize) & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr as usize - segment_addr) / PAGE_SIZE;
        let page = unsafe { &mut (*segment).pages[page_index] };

        assert_eq!(page.alloc_count, 1);
        assert!(page.thread_free.is_empty());

        MemoryBackendWrapper::with_allocator(|alloc_ref| {
            let _borrow = alloc_ref.borrow_mut();
            unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
        });

        assert_eq!(page.alloc_count, 1);
        assert!(!page.thread_free.is_empty());

        let reclaimed = unsafe { page.reclaim_thread_free() };
        assert_eq!(reclaimed, 1);
        assert_eq!(page.alloc_count, 0);
        assert!(page.thread_free.is_empty());
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
        assert!(ptr.is_null());
    }
}
