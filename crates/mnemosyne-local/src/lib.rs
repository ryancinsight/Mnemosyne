//! Thread-local cache allocation and deallocation routing.

#![no_std]
// The `nightly_tls` feature swaps the portable `std::thread_local!` accessor for
// an ELF/PE `#[thread_local]` static, which the unstable `thread_local` language
// feature provides. The default build never enables this and stays on stable.
#![cfg_attr(feature = "nightly_tls", feature(thread_local))]

extern crate std;

pub mod local_alloc;

pub use local_alloc::{SizeClassOccupancy, ThreadAllocator, ThreadAllocatorStats};

use core::ptr::NonNull;
use mnemosyne_arena::{allocate_large_or_huge, deallocate_large_or_huge, HasSegmentPool};
use mnemosyne_core::constants::{MIN_BLOCK_SIZE, PAGES_PER_SEGMENT, PAGE_SIZE, SEGMENT_SIZE};
use mnemosyne_core::size_class::size_to_class;
use mnemosyne_core::types::{Block, Page, Segment};
use mnemosyne_core::validation::{is_valid_alloc_request, is_valid_layout_alloc_request};

use mnemosyne_core::policy::AllocPolicy;

/// Per-thread allocator cache plus reentrancy guard.
///
/// Keeping the guard and cache in a single TLS object makes the allocation
/// fast path pay one thread-local lookup instead of first looking up the guard
/// and then the allocator cache. The guard still enforces the same exclusive
/// borrowing contract as the former split TLS keys.
#[doc(hidden)]
pub struct LocalAllocatorSlot<B: HasSegmentPool> {
    is_allocating: core::cell::Cell<bool>,
    /// One-shot flag recording whether this thread's exit-reclamation sentinel
    /// has been registered. Only the `#[thread_local]` fast path needs it: a
    /// `#[thread_local]` static is not dropped on thread teardown, so the first
    /// hot-path access arms a `std::thread_local!` `Drop` sentinel exactly once.
    #[cfg(feature = "nightly_tls")]
    exit_armed: core::cell::Cell<bool>,
    allocator: core::cell::UnsafeCell<ThreadAllocator<B>>,
}

impl<B: HasSegmentPool> LocalAllocatorSlot<B> {
    /// Creates an empty per-thread allocator slot.
    pub const fn new() -> Self {
        Self {
            is_allocating: core::cell::Cell::new(false),
            #[cfg(feature = "nightly_tls")]
            exit_armed: core::cell::Cell::new(false),
            allocator: core::cell::UnsafeCell::new(ThreadAllocator::new()),
        }
    }

    /// Runs `f` with exclusive access to the per-thread allocator cache.
    ///
    /// Returns `None` when the current thread already holds the allocator
    /// guard, preserving the re-entrant fallback path without exposing the
    /// internal `UnsafeCell` to macro expansion sites.
    #[inline(always)]
    pub fn with_allocator<R>(&self, f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        if self.is_allocating.get() {
            return None;
        }
        self.is_allocating.set(true);
        // Safety: this slot is stored in thread-local storage, so no other
        // thread can access the cell. `is_allocating` rejects nested access on
        // the same thread before a second mutable reference can be created.
        let result = unsafe { f(&mut *self.allocator.get()) };
        self.is_allocating.set(false);
        Some(result)
    }

    /// Runs `f` with `&mut` access to the cache **without** arming the
    /// re-entrancy guard, returning `None` when a guarded operation is already
    /// in progress on this thread.
    ///
    /// This is the sound primitive behind the guard-free small-allocation fast
    /// path. It still reads `is_allocating`, so it can never hand out a second
    /// `&mut ThreadAllocator` while a guarded borrow is live — it simply skips
    /// the `set(true)`/`set(false)` writes that bracket [`with_allocator`].
    /// Because it does not arm the guard, the borrow it creates is only sound
    /// if `f` performs no operation that can re-enter the allocator (no segment
    /// acquisition, no backend call, no foreign callback). Callers use it for
    /// the active-page free-list pop, which touches only thread-local page
    /// metadata and never allocates.
    ///
    /// # Safety
    ///
    /// `f` must not, directly or transitively, invoke any allocator entry point
    /// on the current thread (which would create an aliasing `&mut` to this
    /// same cache).
    #[inline(always)]
    pub unsafe fn with_allocator_unguarded<R>(
        &self,
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        if self.is_allocating.get() {
            return None;
        }
        // Safety: `is_allocating` is false, so no guarded `&mut` to this cache
        // is live on this thread; the slot is thread-local, so no other thread
        // aliases it; and the caller's `f` contract forbids re-entry, so no
        // nested `&mut` can be created while this borrow is held.
        Some(unsafe { f(&mut *self.allocator.get()) })
    }

    /// Returns the raw allocator-cache pointer used as the segment owner token.
    #[inline(always)]
    pub fn allocator_ptr(&self) -> *mut core::ffi::c_void {
        self.allocator.get().cast()
    }

    /// Returns the typed cache pointer for thread-exit reclamation binding.
    #[cfg(feature = "nightly_tls")]
    #[inline(always)]
    fn cache_ptr(&self) -> *mut ThreadAllocator<B> {
        self.allocator.get()
    }
}

/// Thread-exit reclamation sentinel for the `#[thread_local]` fast cache.
///
/// A `#[thread_local]` static does not run `Drop` when its owning thread exits,
/// so the segment-reclamation logic in `ThreadAllocator::reclaim_owned_segments`
/// would never fire and every terminated worker would leak its owned segments.
/// This sentinel restores that guarantee: it is a standard `std::thread_local!`
/// value (which *is* dropped at thread exit) holding a raw pointer to the
/// thread's `#[thread_local]` allocator cache. The first hot-path access binds
/// the pointer; thread teardown invokes `Drop`, which reclaims the segments.
#[cfg(feature = "nightly_tls")]
#[doc(hidden)]
pub struct ThreadExitReclaim<B: HasSegmentPool> {
    cache: core::cell::Cell<*mut ThreadAllocator<B>>,
}

#[cfg(feature = "nightly_tls")]
impl<B: HasSegmentPool> ThreadExitReclaim<B> {
    /// Creates an unbound sentinel.
    pub const fn new() -> Self {
        Self {
            cache: core::cell::Cell::new(core::ptr::null_mut()),
        }
    }

    #[inline(always)]
    fn bind(&self, cache: *mut ThreadAllocator<B>) {
        self.cache.set(cache);
    }
}

#[cfg(feature = "nightly_tls")]
impl<B: HasSegmentPool> Default for ThreadExitReclaim<B> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "nightly_tls")]
impl<B: HasSegmentPool> Drop for ThreadExitReclaim<B> {
    fn drop(&mut self) {
        let cache = self.cache.get();
        if !cache.is_null() {
            // Safety: `cache` was bound to the address of this thread's
            // `#[thread_local]` allocator slot, whose storage outlives every
            // standard thread-local destructor on the same thread. The slot is
            // exclusive to this thread and `reclaim_owned_segments` clears the
            // owned-segment head, so the operation is single-shot and unaliased.
            unsafe {
                (*cache).reclaim_owned_segments();
            }
        }
    }
}

/// Registers the thread-exit reclamation sentinel on first use (idempotent).
///
/// The check reads a flag inside the `#[thread_local]` slot itself (a single
/// segment-relative load), so the steady-state hot path never touches the
/// `std::thread_local!` accessor that backs the sentinel.
#[cfg(feature = "nightly_tls")]
#[inline(always)]
pub fn arm_thread_exit<B: HasSegmentPool>(
    slot: &LocalAllocatorSlot<B>,
    guard: &'static std::thread::LocalKey<ThreadExitReclaim<B>>,
) {
    if !slot.exit_armed.get() {
        cold_arm_thread_exit(slot, guard);
    }
}

#[cfg(feature = "nightly_tls")]
#[cold]
#[inline(never)]
fn cold_arm_thread_exit<B: HasSegmentPool>(
    slot: &LocalAllocatorSlot<B>,
    guard: &'static std::thread::LocalKey<ThreadExitReclaim<B>>,
) {
    slot.exit_armed.set(true);
    guard.with(|sentinel| sentinel.bind(slot.cache_ptr()));
}

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

    /// Runs `f` with the thread-local allocator cache **without** arming the
    /// re-entrancy guard, returning `None` on same-thread re-entry.
    ///
    /// This backs the guard-free small-allocation fast path: it still consults
    /// the re-entrancy busy bit (so it never produces a second `&mut` while a
    /// guarded borrow is live) but skips the guard set/clear writes.
    ///
    /// # Safety
    ///
    /// `f` must not, directly or transitively, invoke any allocator entry point
    /// on the current thread.
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R>;

    /// Returns the raw pointer to the thread-local allocator cache.
    fn get_allocator_ptr() -> *mut core::ffi::c_void;
}

/// Helper macro to generate zero-cost backend-specific thread-local cache pools.
#[macro_export]
macro_rules! impl_local_allocator_selector {
    ($backend:ty) => {
        // Default portable path: `std::thread_local!` with a `const {}`
        // initializer (stable since Rust 1.59). Because `LocalAllocatorSlot::new`
        // is a `const fn`, the compiler emits the fast accessor that skips the
        // per-access lazy-initialization guard branch, and the slot still
        // registers its `Drop` destructor for thread-exit segment reclamation.
        #[cfg(not(feature = "nightly_tls"))]
        const _: () = {
            std::thread_local! {
                static ALLOCATOR_SLOT: $crate::LocalAllocatorSlot<$backend> = const {
                    $crate::LocalAllocatorSlot::new()
                };
            }

            impl $crate::LocalAllocatorSelector<$backend> for $backend {
                #[inline(always)]
                fn with_allocator<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    ALLOCATOR_SLOT.with(|slot| slot.with_allocator(f))
                }

                #[inline(always)]
                fn with_allocator_guard<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    ALLOCATOR_SLOT.with(|slot| slot.with_allocator(f))
                }

                #[inline(always)]
                unsafe fn with_allocator_unguarded<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    // Safety: the caller upholds the no-re-entry contract; the
                    // slot method checks the busy bit before borrowing.
                    ALLOCATOR_SLOT.with(|slot| unsafe { slot.with_allocator_unguarded(f) })
                }

                #[inline(always)]
                fn get_allocator_ptr() -> *mut core::ffi::c_void {
                    ALLOCATOR_SLOT.with(|slot| slot.allocator_ptr())
                }
            }
        };

        // Fast path: an ELF/PE `#[thread_local]` static. Accessing it lowers to
        // a single segment-register-relative load (e.g. `%fs:OFFSET`) with no
        // `LocalKey::with` call and no lazy-init guard — the same mechanism
        // mimalloc uses for its default heap. A separate `std::thread_local!`
        // sentinel (`ALLOCATOR_EXIT_GUARD`) preserves thread-exit segment
        // reclamation because `#[thread_local]` statics are not auto-dropped.
        #[cfg(feature = "nightly_tls")]
        const _: () = {
            #[thread_local]
            static ALLOCATOR_SLOT: $crate::LocalAllocatorSlot<$backend> =
                $crate::LocalAllocatorSlot::new();

            std::thread_local! {
                static ALLOCATOR_EXIT_GUARD: $crate::ThreadExitReclaim<$backend> = const {
                    $crate::ThreadExitReclaim::new()
                };
            }

            impl $crate::LocalAllocatorSelector<$backend> for $backend {
                #[inline(always)]
                fn with_allocator<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    $crate::arm_thread_exit(&ALLOCATOR_SLOT, &ALLOCATOR_EXIT_GUARD);
                    ALLOCATOR_SLOT.with_allocator(f)
                }

                #[inline(always)]
                fn with_allocator_guard<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    $crate::arm_thread_exit(&ALLOCATOR_SLOT, &ALLOCATOR_EXIT_GUARD);
                    ALLOCATOR_SLOT.with_allocator(f)
                }

                #[inline(always)]
                unsafe fn with_allocator_unguarded<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    // Safety: the caller upholds the no-re-entry contract. The
                    // exit sentinel is already armed: the first allocation on a
                    // thread has no active page and necessarily takes the
                    // guarded path, which arms the sentinel before any segment
                    // is owned, so the unguarded fast path need not re-arm.
                    unsafe { ALLOCATOR_SLOT.with_allocator_unguarded(f) }
                }

                #[inline(always)]
                fn get_allocator_ptr() -> *mut core::ffi::c_void {
                    ALLOCATOR_SLOT.allocator_ptr()
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
#[inline]
pub unsafe fn usable_size(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }

    let segment_addr = (ptr as usize) & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr as usize - segment_addr) / PAGE_SIZE;

    // Safety: for small allocations, page_index is in [1, PAGES_PER_SEGMENT)
    // and the target page records the size-class block size. If page_index is
    // 0 (segment-aligned huge allocation) or the page's block_size is 0
    // (non-segment-aligned huge allocation), we route to the metadata-slot fallback.
    if page_index > 0 {
        let page = unsafe { (*segment).pages.get_unchecked(page_index) };
        let size = page.block_size;
        if size > 0 {
            return size;
        }
    }

    // Safety: large/huge allocations store the segment pointer in the metadata
    // slot immediately preceding the user pointer.
    let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
    unsafe { (*segment).huge_mapping_suffix_from(ptr) }
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
    if align > MIN_BLOCK_SIZE {
        // Fall back to direct arena/huge allocation to break recursion and get high alignment (64KB aligned page-starts).
        // Safety: allocate_large_or_huge handles safety.
        let ptr = unsafe { allocate_large_or_huge::<B>(size, align) };
        if !ptr.is_null() {
            unsafe { initialize_allocated_bytes::<P>(ptr, size) };
        }
        return ptr;
    }

    let adjusted_size = if size < align { align } else { size };

    let class = match size_to_class(adjusted_size) {
        Some(c) => c,
        None => {
            let ptr = unsafe { allocate_large_or_huge::<B>(adjusted_size, align) };
            if !ptr.is_null() {
                unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
            }
            return ptr;
        }
    };

    // Guard-free small-allocation fast path. `with_allocator_unguarded` still
    // consults the re-entrancy busy bit — returning `None` on same-thread
    // re-entry so a second `&mut ThreadAllocator` can never alias a live
    // guarded borrow — but skips the guard set/clear writes. The closure only
    // pops a thread-local free-list block and performs no allocator re-entry,
    // satisfying the unguarded-borrow contract. A re-entrant call (busy bit
    // set) falls through to `with_allocator_guard`, which also returns `None`
    // and routes to the huge fallback.
    //
    let pop_active_free = |alloc: &mut ThreadAllocator<B>| -> Option<*mut u8> {
        let mut page_ptr = unsafe { *alloc.active_pages.get_unchecked(class) }?;
        // Safety: `page_ptr` is an active page owned by this thread cache.
        let page = unsafe { page_ptr.as_mut() };
        let block = page.free?;
        // Safety: `block` is the head of the page-local free list; advance the
        // list and account the allocation.
        unsafe {
            page.free = (*block.as_ptr()).next;
        }
        page.alloc_count += 1;
        Some(block.as_ptr() as *mut u8)
    };
    // Safety: `pop_active_free` touches only thread-local page metadata and
    // never invokes an allocator entry point, upholding the no-re-entry
    // contract of `with_allocator_unguarded`.
    let fast = unsafe { B::with_allocator_unguarded(pop_active_free) };
    if let Some(Some(ptr)) = fast {
        unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
        return ptr;
    }

    // Fallback: enter allocator guard for cold allocations.
    let Some(ptr) = B::with_allocator_guard(|alloc| {
        // Safety: alloc is exclusively borrowed and valid.
        unsafe { alloc.alloc_cold(class) }
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
            // The poison region runs from `ptr` to the end of the OS-side
            // mapping (`raw_alloc_ptr + huge_size`), not to
            // `segment + huge_size` — the latter sits up to SEGMENT_ALIGN-1
            // bytes past the mapping end and would write across the OS
            // boundary.
            let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            let size = unsafe { (*segment).huge_mapping_suffix_from(ptr) };
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
    let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };
    if page.block_size == 0 {
        if P::ENABLE_POISONING {
            // Safety: see the segment-aligned branch above for the
            // raw_alloc_ptr derivation; using segment_ptr here would
            // write `aligned_addr - raw_alloc_ptr` bytes past the OS
            // mapping boundary.
            let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            let size = unsafe { (*segment).huge_mapping_suffix_from(ptr) };
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

        #[inline(always)]
        unsafe fn do_local_free<B: HasSegmentPool>(
            alloc: &mut ThreadAllocator<B>,
            block: *mut Block,
            page: &mut Page,
            segment: *mut Segment,
        ) {
            (*block).next = page.free;
            page.free = Some(NonNull::new_unchecked(block));
            let was_full = page.alloc_count == page.max_blocks;
            if page.alloc_count > 0 {
                page.alloc_count -= 1;
            }
            if was_full {
                let class = page.size_class;
                if alloc.unlink_full_page(page as *mut Page, class) {
                    page.next_page = unsafe { *alloc.active_pages.get_unchecked(class) };
                    unsafe { *alloc.active_pages.get_unchecked_mut(class) = Some(NonNull::new_unchecked(page as *mut Page)); }
                }
            }
            if page.alloc_count == 0 && !alloc.is_current_segment(segment) {
                if !alloc.try_reclaim_segment(segment) {
                    let class = page.size_class;
                    alloc.unlink_page(page as *mut Page, class);
                    page.next_page = alloc.empty_pages;
                    alloc.empty_pages = Some(NonNull::new_unchecked(page as *mut Page));
                }
            }
        }

        // Safety: the closure only touches thread-local page metadata and
        // unlinks/reclaims owned segments, which involves no nested allocator
        // entry points, satisfying the unguarded safety contract.
        let fast_reclaim = unsafe {
            B::with_allocator_unguarded(|alloc| do_local_free(alloc, block, page, segment))
        };
        let res = if fast_reclaim.is_none() {
            // Reentrant fallback
            B::with_allocator_guard(|alloc| unsafe { do_local_free(alloc, block, page, segment) })
        } else {
            fast_reclaim
        };

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
    fn usable_size_never_under_reports_across_every_size_class() {
        // The lower-bound counterpart to
        // `usable_size_does_not_over_report_past_mapping_end_for_huge_allocations`.
        // An under-report is the more dangerous direction for small
        // allocations: a `Vec` that trusts `usable_size` to compute spare
        // capacity would write past the reported window and corrupt an
        // adjacent block. Exhaustively prove `usable_size(ptr) >=
        // requested_size` for at least one representative request in every
        // small size class, plus the inter-class boundary bytes that the
        // size-class mapper rounds.
        use mnemosyne_core::size_class::class_to_size;
        use mnemosyne_core::NUM_SIZE_CLASSES;

        for class in 0..NUM_SIZE_CLASSES {
            let class_max = class_to_size(class);
            // Exercise the smallest request that lands in this class
            // (one byte past the previous class's max) and the class max
            // itself. Both must report at least the requested size.
            let prev_max = if class == 0 {
                0
            } else {
                class_to_size(class - 1)
            };
            for &req in &[prev_max + 1, class_max] {
                let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(req, 8) };
                assert!(
                    !ptr.is_null(),
                    "alloc({req}) returned null for class {class}"
                );

                let reported = unsafe { usable_size(ptr) };
                assert!(
                    reported >= req,
                    "usable_size under-reported for class {class}: requested {req}, got {reported}"
                );
                // The reported value is the class block size, which must
                // be exactly `class_max` for any request in this class.
                assert_eq!(
                    reported, class_max,
                    "usable_size for request {req} (class {class}) should equal class max {class_max}"
                );

                unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
            }
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
    fn usable_size_does_not_over_report_past_mapping_end_for_huge_allocations() {
        // Strict assertion that catches the SEGMENT_ALIGN-1 byte over-report
        // that resulted from using segment_ptr (aligned_addr) as the
        // mapping base instead of segment.raw_alloc_ptr. We compute the
        // distance from ptr to the end of the *actual* OS mapping
        // (raw_alloc_ptr + huge_size) and assert usable_size never exceeds it.
        let request = 4 * 1024 * 1024;
        for &align in &[8usize, 64 * 1024, 1024 * 1024, SEGMENT_SIZE] {
            // Safety: power-of-two alignment, non-zero size.
            let ptr = unsafe {
                mnemosyne_arena::allocate_large_or_huge::<MemoryBackendWrapper>(request, align)
            };
            assert!(!ptr.is_null(), "huge allocation failed for align {align}");

            let recovered = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            let huge_size = unsafe { (*recovered).pages[0].block_size };
            let raw_ptr = unsafe { (*recovered).raw_alloc_ptr } as usize;
            let mapping_end = raw_ptr + huge_size;
            let actual_remaining = mapping_end - ptr as usize;

            let reported = unsafe { usable_size(ptr) };
            assert!(
                reported <= actual_remaining,
                "usable_size {} exceeds remaining mapping {} (raw_ptr={:#x}, ptr={:?}, huge_size={}) for align {align}",
                reported,
                actual_remaining,
                raw_ptr,
                ptr,
                huge_size,
            );
            assert!(
                reported >= request,
                "usable_size {} is below requested {} for align {align}",
                reported,
                request,
            );

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
