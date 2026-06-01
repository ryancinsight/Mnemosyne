use crate::ThreadAllocator;
use mnemosyne_arena::HasSegmentPool;

/// Per-thread allocator cache plus reentrancy guard.
///
/// Keeping the guard and cache in a single TLS object makes the allocation
/// fast path pay one thread-local lookup instead of first looking up the guard
/// and then the allocator cache. The guard still enforces the same exclusive
/// borrowing contract as the former split TLS keys.
#[doc(hidden)]
#[repr(C)]
pub struct LocalAllocatorSlot<B: HasSegmentPool> {
    allocator: core::cell::UnsafeCell<ThreadAllocator<B>>,
    /// One-shot flag recording whether this thread's exit-reclamation sentinel
    /// has been registered. Only the `#[thread_local]` fast path needs it: a
    /// `#[thread_local]` static is not dropped on thread teardown, so the first
    /// hot-path access arms a `std::thread_local!` `Drop` sentinel exactly once.
    #[cfg(feature = "nightly_tls")]
    pub(crate) exit_armed: core::cell::Cell<bool>,
}

impl<B: HasSegmentPool> Default for LocalAllocatorSlot<B> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<B: HasSegmentPool> LocalAllocatorSlot<B> {
    /// Creates an empty per-thread allocator slot.
    pub const fn new() -> Self {
        Self {
            allocator: core::cell::UnsafeCell::new(ThreadAllocator::new()),
            #[cfg(feature = "nightly_tls")]
            exit_armed: core::cell::Cell::new(false),
        }
    }

    /// Runs `f` with exclusive access to the per-thread allocator cache.
    ///
    /// Returns `None` when the current thread already holds the allocator
    /// guard, preserving the re-entrant fallback path without exposing the
    /// internal `UnsafeCell` to macro expansion sites.
    #[inline(always)]
    pub fn with_allocator<R>(&self, f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        let alloc = unsafe { &mut *self.allocator.get() };
        if alloc.is_allocating {
            return None;
        }
        alloc.is_allocating = true;
        // Safety: this slot is stored in thread-local storage, so no other
        // thread can access the cell. `is_allocating` rejects nested access on
        // the same thread before a second mutable reference can be created.
        let result = f(alloc);
        alloc.is_allocating = false;
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
        let alloc = unsafe { &mut *self.allocator.get() };
        if alloc.is_allocating {
            return None;
        }
        // Safety: `is_allocating` is false, so no guarded `&mut` to this cache
        // is live on this thread; the slot is thread-local, so no other thread
        // aliases it; and the caller's `f` contract forbids re-entry, so no
        // nested `&mut` can be created while this borrow is held.
        Some(f(alloc))
    }

    /// Returns the raw allocator-cache pointer used as the segment owner token.
    #[inline(always)]
    pub fn allocator_ptr(&self) -> *mut core::ffi::c_void {
        self.allocator.get().cast()
    }

    /// Returns the typed cache pointer for thread-exit reclamation binding.
    #[cfg(feature = "nightly_tls")]
    #[inline(always)]
    pub fn cache_ptr(&self) -> *mut ThreadAllocator<B> {
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
    pub fn bind(&self, cache: *mut ThreadAllocator<B>) {
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

    /// Returns the raw pointer to the thread-local allocator cache without triggering lazy initialization.
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void;
}
