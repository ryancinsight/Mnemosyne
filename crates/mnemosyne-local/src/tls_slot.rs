use crate::ThreadAllocator;
use mnemosyne_arena::HasSegmentPool;

/// Per-thread allocator cache plus reentrancy guard.
///
/// Keeping the guard and cache in a single TLS object makes the allocation
/// fast path pay one thread-local lookup instead of first looking up the guard
/// and then the allocator cache. The guard still enforces the same exclusive
/// borrowing contract as the former split TLS keys.
#[doc(hidden)]
/// `repr(C)` with `allocator` first is load-bearing, not cosmetic.
///
/// The TLS fast paths cache one pointer, and that same address is the segment
/// **owner token** compared by `SegmentOwner::matches` on every free. The
/// re-entrancy gate must be reachable from that cached pointer, but it cannot
/// live *inside* `ThreadAllocator` (see `is_allocating` below). Fixing the
/// allocator at offset 0 makes the slot address and the allocator address the
/// same value, so the cache can hold a slot-provenanced pointer — reaching the
/// gate — while every owner-token comparison sees the identical value it saw
/// before. `SLOT_ALLOCATOR_AT_OFFSET_ZERO` fails the build if that stops
/// holding.
#[repr(C)]
pub struct LocalAllocatorSlot<B: HasSegmentPool> {
    allocator: core::cell::UnsafeCell<ThreadAllocator<B>>,
    /// Re-entrancy gate for `allocator`.
    ///
    /// A *sibling* of the allocator rather than a field inside it. The flag
    /// decides whether forming `&mut ThreadAllocator` is legal, so it cannot
    /// live in the memory that borrow covers: on a re-entrant call the outer
    /// `&mut` is live and strongly protected, and a protected borrow excludes
    /// every access through another tag — including a read of one field.
    /// Testing the flag inside the allocator therefore commits the exact
    /// aliasing it exists to reject, which is what Miri reported from
    /// `unguarded_fast_path_rejects_reentrant_borrow`. As a sibling it lies
    /// outside the borrowed range and can always be read.
    is_allocating: core::cell::Cell<bool>,
    pub(crate) os_key: core::cell::Cell<u32>,
    /// One-shot flag recording whether this thread's exit-reclamation sentinel
    /// has been registered. Only the `#[thread_local]` fast path needs it: a
    /// `#[thread_local]` static is not dropped on thread teardown, so the first
    /// hot-path access arms a `std::thread_local!` `Drop` sentinel exactly once.
    #[cfg(nightly_tls_active)]
    pub(crate) exit_armed: core::cell::Cell<bool>,
}

impl<B: HasSegmentPool> Default for LocalAllocatorSlot<B> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<B: HasSegmentPool> LocalAllocatorSlot<B> {
    /// Pins the layout the owner-token identity depends on.
    ///
    /// If `allocator` ever stops being at offset 0, the cached slot pointer and
    /// the allocator address diverge, every `SegmentOwner::matches` on the free
    /// path starts failing, and self-frees silently misroute as cross-thread
    /// frees. That is a correctness bug with no memory-safety symptom, so it is
    /// caught at compile time instead.
    const SLOT_ALLOCATOR_AT_OFFSET_ZERO: () = assert!(
        core::mem::offset_of!(Self, allocator) == 0,
        "LocalAllocatorSlot::allocator must stay at offset 0: the TLS cache holds one          pointer that is both the slot address and the segment owner token"
    );

    /// Borrows only the re-entrancy gate's own byte from a cached slot pointer.
    ///
    /// The `&raw` place projection is load-bearing. Forming a `&Self` here and
    /// reading the field through it would retag the *whole* slot, allocator
    /// included — and the gate exists precisely to be readable while a
    /// `&mut ThreadAllocator` derived from this slot is live and protected, so
    /// that overlap is the very aliasing it is meant to detect. Projecting
    /// through a raw place creates no intermediate reference, so the retag
    /// covers one byte outside the allocator's range.
    ///
    /// # Safety
    ///
    /// `ptr` must be the address of a live `LocalAllocatorSlot` of this thread,
    /// as produced by [`LocalAllocatorSlot::allocator_ptr`] (which, by the
    /// offset-0 invariant above, is also the slot address).
    #[inline(always)]
    unsafe fn gate<'a>(ptr: *mut core::ffi::c_void) -> &'a core::cell::Cell<bool> {
        const { Self::SLOT_ALLOCATOR_AT_OFFSET_ZERO };
        let slot = ptr.cast::<Self>();
        // SAFETY: caller guarantees `ptr` is this thread's live slot address;
        // the projection reads no memory and yields a pointer to the gate.
        let gate = unsafe { &raw const (*slot).is_allocating };
        // SAFETY: the gate is a live `Cell<bool>` inside that slot, and the slot
        // is thread-affine, so a shared borrow of it cannot race.
        unsafe { &*gate }
    }

    /// Reads the re-entrancy gate without borrowing the allocator.
    ///
    /// # Safety
    ///
    /// Same contract as [`LocalAllocatorSlot::gate`].
    #[inline(always)]
    pub unsafe fn is_allocating(ptr: *mut core::ffi::c_void) -> bool {
        // SAFETY: forwarded to the caller.
        unsafe { Self::gate(ptr) }.get()
    }

    /// Raises or lowers the re-entrancy gate.
    ///
    /// Callers that hand out a `&mut ThreadAllocator` themselves — rather than
    /// going through [`LocalAllocatorSlot::with_allocator`] — must bracket that
    /// borrow with this.
    ///
    /// # Safety
    ///
    /// Same contract as [`LocalAllocatorSlot::gate`].
    #[inline(always)]
    pub unsafe fn set_allocating(ptr: *mut core::ffi::c_void, value: bool) {
        // SAFETY: forwarded to the caller.
        unsafe { Self::gate(ptr) }.set(value);
    }

    /// Borrows the allocator cache exclusively from a cached slot pointer.
    ///
    /// Sound only while the gate reads `false`; callers check it first.
    ///
    /// # Safety
    ///
    /// Same contract as [`LocalAllocatorSlot::gate`], and no other borrow of
    /// this thread's allocator cache may be live.
    #[inline(always)]
    pub unsafe fn allocator_mut<'a>(ptr: *mut core::ffi::c_void) -> &'a mut ThreadAllocator<B> {
        const { Self::SLOT_ALLOCATOR_AT_OFFSET_ZERO };
        // SAFETY: the allocator sits at offset 0 of the slot (asserted above),
        // so the slot address is the cache address, and the caller guarantees
        // both liveness and the absence of any other live borrow. The retag
        // covers the allocator's range only, leaving the gate byte untouched.
        unsafe { &mut *ptr.cast::<ThreadAllocator<B>>() }
    }

    /// Creates an empty per-thread allocator slot.
    pub const fn new() -> Self {
        Self {
            allocator: core::cell::UnsafeCell::new(ThreadAllocator::new()),
            is_allocating: core::cell::Cell::new(false),
            os_key: core::cell::Cell::new(u32::MAX),
            #[cfg(nightly_tls_active)]
            exit_armed: core::cell::Cell::new(false),
        }
    }

    /// Runs `f` with exclusive access to the per-thread allocator cache.
    ///
    /// Returns `None` when the current thread already holds the allocator
    /// guard, preserving the re-entrant fallback path without exposing the
    /// internal `UnsafeCell` to macro expansion sites.
    /// # Safety
    ///
    /// Same contract as [`LocalAllocatorSlot::gate`].
    #[inline(always)]
    pub unsafe fn with_allocator<R>(
        ptr: *mut core::ffi::c_void,
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        // The gate is read before any allocator reference exists: it lives
        // beside the allocator, not inside it, precisely so this check cannot
        // touch memory an outer borrow protects.
        // SAFETY: forwarded to the caller.
        if unsafe { Self::is_allocating(ptr) } {
            return None;
        }
        // SAFETY: forwarded to the caller.
        unsafe { Self::set_allocating(ptr, true) };
        // SAFETY: this slot is thread-affine, so no other thread can reach the
        // cache, and the gate above rejected nested access on this thread
        // before any second mutable reference could be created.
        let alloc = unsafe { Self::allocator_mut(ptr) };
        let result = f(alloc);
        // SAFETY: forwarded to the caller.
        unsafe { Self::set_allocating(ptr, false) };
        Some(result)
    }

    /// Runs `f` with `&mut` access to the cache **without** arming the
    /// re-entrancy guard, returning `None` when a guarded operation is already
    /// in progress on this thread.
    ///
    /// This is the sound primitive behind the guard-free small-allocation fast
    /// path. It still reads the gate, so it can never hand out a second
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
    /// Same contract as [`LocalAllocatorSlot::gate`], and `f` must not,
    /// directly or transitively, invoke any allocator entry point on the
    /// current thread (which would create an aliasing `&mut` to this cache).
    #[inline(always)]
    pub unsafe fn with_allocator_unguarded<R>(
        ptr: *mut core::ffi::c_void,
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        // Gate first, borrow second — see `with_allocator`.
        // SAFETY: forwarded to the caller.
        if unsafe { Self::is_allocating(ptr) } {
            return None;
        }
        // SAFETY: the gate is false, so no guarded `&mut` to this cache is live
        // on this thread; the slot is thread-affine, so no other thread aliases
        // it; and the caller's `f` contract forbids re-entry, so no nested
        // `&mut` can be created while this borrow is held.
        let alloc = unsafe { Self::allocator_mut(ptr) };
        Some(f(alloc))
    }

    /// Returns the raw allocator-cache pointer used as the segment owner token.
    #[inline(always)]
    pub fn allocator_ptr(&self) -> *mut core::ffi::c_void {
        const { Self::SLOT_ALLOCATOR_AT_OFFSET_ZERO };
        // Derived from the *slot*, not from `self.allocator.get()`. Both yield
        // the same address (the offset-0 invariant), so every owner-token
        // comparison is unaffected — but only the slot-derived pointer carries
        // provenance over the whole slot, which the re-entrancy gate needs: the
        // gate is a sibling field past the allocator, and a pointer whose
        // provenance covers only the `UnsafeCell` cannot reach it. Miri rejects
        // that as a retag outside the borrow stack.
        (self as *const Self as *mut Self).cast()
    }

    /// Returns the typed cache pointer for thread-exit reclamation binding.
    #[cfg(nightly_tls_active)]
    #[inline(always)]
    pub fn cache_ptr(&self) -> *mut ThreadAllocator<B> {
        self.allocator.get()
    }
}

impl<B: HasSegmentPool> Drop for LocalAllocatorSlot<B> {
    #[inline]
    fn drop(&mut self) {
        let key = self.os_key.get();
        if key != u32::MAX {
            #[cfg(all(windows, target_arch = "x86_64", not(miri)))]
            // SAFETY: `key != u32::MAX` was set by this thread when it published
            // its allocator pointer (`os_key.set(key)`), so it is a valid
            // `TlsAlloc` key. This slot is dropped on its owning thread, so the
            // write clears the current thread's own TEB slot, severing the now-
            // dangling cached pointer to the slot being destroyed.
            unsafe {
                crate::tls::os_helpers::set_teb_tls_slot(key, core::ptr::null_mut());
            }
            crate::tls::os_helpers::set_os_tls_value(key, core::ptr::null_mut());
        }
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
#[cfg(nightly_tls_active)]
#[doc(hidden)]
pub struct ThreadExitReclaim<B: HasSegmentPool> {
    cache: core::cell::Cell<*mut ThreadAllocator<B>>,
}

#[cfg(nightly_tls_active)]
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

#[cfg(nightly_tls_active)]
impl<B: HasSegmentPool> Default for ThreadExitReclaim<B> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(nightly_tls_active)]
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
#[cfg(nightly_tls_active)]
#[inline(always)]
pub fn arm_thread_exit<B: HasSegmentPool>(
    slot: &LocalAllocatorSlot<B>,
    guard: &'static std::thread::LocalKey<ThreadExitReclaim<B>>,
) {
    if !slot.exit_armed.get() {
        cold_arm_thread_exit(slot, guard);
    }
}

#[cfg(nightly_tls_active)]
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
///
/// Implementors provide independent cache state for the standard and
/// encrypted free-list modes. The exported selector macro is the canonical
/// implementation; custom implementations must preserve the same ownership
/// and mode-isolation contract.
pub trait LocalAllocatorSelector<B: HasSegmentPool>: HasSegmentPool {
    /// Evaluates the closure with a mutable reference to the thread-local allocator cache,
    /// arming the re-entrancy guard.
    ///
    /// Returns `None` if the allocator is already borrowed (re-entrancy detected).
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R>;

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

    /// Runs `f` against the TLS allocator selected by the compile-time
    /// free-list encryption mode.
    ///
    /// The mode is part of the selector call, not process-global state. This
    /// gives each `(backend, encryption mode)` pair an independent allocator
    /// cache while preserving static dispatch and the existing backend seam.
    fn with_allocator_for_policy<P: mnemosyne_core::AllocPolicy, R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R>;

    /// Mode-keyed counterpart of [`Self::with_allocator_unguarded`].
    ///
    /// # Safety
    ///
    /// `f` must not, directly or transitively, invoke an allocator entry point
    /// on the current thread.
    unsafe fn with_allocator_unguarded_for_policy<P: mnemosyne_core::AllocPolicy, R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R>;

    /// Returns the initialized mode-keyed allocator pointer, arming its TLS
    /// slot when necessary.
    fn get_allocator_ptr_for_policy<P: mnemosyne_core::AllocPolicy>() -> *mut core::ffi::c_void;

    /// Returns the initialized mode-keyed allocator pointer without creating
    /// its slot.
    fn get_allocator_ptr_raw_for_policy<P: mnemosyne_core::AllocPolicy>() -> *mut core::ffi::c_void;

    /// Returns the raw pointer for a statically selected free-list encoding
    /// mode without creating its slot.
    fn get_allocator_ptr_raw_for_encryption<const ENCRYPTED: bool>() -> *mut core::ffi::c_void;
}
