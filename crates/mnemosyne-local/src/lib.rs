//! Thread-local cache allocation and deallocation routing.

#![no_std]
// The `nightly_tls` feature swaps the portable `std::thread_local!` accessor for
// an ELF/PE `#[thread_local]` static, which the unstable `thread_local` language
// feature provides. The default build never enables this and stays on stable.
#![cfg_attr(feature = "nightly_tls", feature(thread_local))]

extern crate std;

pub mod local_alloc;
pub mod per_cpu;
pub mod tls;

pub use local_alloc::{SizeClassOccupancy, ThreadAllocator, ThreadAllocatorStats};

use core::alloc::Layout;
use core::ptr::NonNull;
use mnemosyne_arena::{allocate_large_or_huge, deallocate_large_or_huge, HasSegmentPool};
use mnemosyne_core::constants::{
    MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, PAGES_PER_SEGMENT, PAGE_SHIFT, PAGE_SIZE, SEGMENT_SIZE,
};
use mnemosyne_core::size_class::size_to_class_nonzero;
use mnemosyne_core::types::{Block, Page, Segment};
use mnemosyne_core::validation::{is_valid_alloc_request, is_valid_layout_alloc_request};

use mnemosyne_core::policy::AllocPolicy;

static OPTIONS_INIT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

#[cfg(windows)]
fn get_env_var_stack(name: &str, buf: &mut [u8]) -> Option<usize> {
    extern "system" {
        fn GetEnvironmentVariableA(lpName: *const u8, lpBuffer: *mut u8, nSize: u32) -> u32;
    }

    let mut name_buf = [0u8; 64];
    if name.len() >= name_buf.len() {
        return None;
    }
    name_buf[..name.len()].copy_from_slice(name.as_bytes());
    name_buf[name.len()] = 0;

    let res =
        unsafe { GetEnvironmentVariableA(name_buf.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };

    if res == 0 || res >= buf.len() as u32 {
        None
    } else {
        Some(res as usize)
    }
}

#[cfg(not(windows))]
fn get_env_var_stack(name: &str, buf: &mut [u8]) -> Option<usize> {
    extern "C" {
        fn getenv(name: *const u8) -> *mut u8;
    }

    let mut name_buf = [0u8; 64];
    if name.len() >= name_buf.len() {
        return None;
    }
    name_buf[..name.len()].copy_from_slice(name.as_bytes());
    name_buf[name.len()] = 0;

    let ptr = unsafe { getenv(name_buf.as_ptr()) };
    if ptr.is_null() {
        return None;
    }

    let mut len = 0;
    unsafe {
        while *ptr.add(len) != 0 && len < buf.len() {
            buf[len] = *ptr.add(len);
            len += 1;
        }
    }
    if len == buf.len() {
        None
    } else {
        Some(len)
    }
}

fn parse_env_usize(name: &str) -> Option<usize> {
    let mut buf = [0u8; 32];
    let len = get_env_var_stack(name, &mut buf)?;
    let s = core::str::from_utf8(&buf[..len]).ok()?;
    s.trim().parse::<usize>().ok()
}

fn parse_env_bool(name: &str) -> Option<bool> {
    let mut buf = [0u8; 32];
    let len = get_env_var_stack(name, &mut buf)?;
    let s = core::str::from_utf8(&buf[..len]).ok()?.trim();
    if s.eq_ignore_ascii_case("true") || s == "1" {
        Some(true)
    } else if s.eq_ignore_ascii_case("false") || s == "0" {
        Some(false)
    } else {
        None
    }
}

#[inline(always)]
pub fn ensure_options_initialized() {
    if !OPTIONS_INIT.load(core::sync::atomic::Ordering::Acquire) {
        init_options_from_env();
    }
}

#[cold]
#[inline(never)]
fn init_options_from_env() {
    if OPTIONS_INIT.swap(true, core::sync::atomic::Ordering::Acquire) {
        return;
    }

    if let Some(parsed) = parse_env_usize("MNEMOSYNE_MAX_RETAINED_SEGMENTS") {
        let clamped = core::cmp::min(parsed, 1024);
        mnemosyne_core::options::MAX_RETAINED_SEGMENTS
            .store(clamped, core::sync::atomic::Ordering::Release);
    }

    if let Some(parsed) = parse_env_bool("MNEMOSYNE_ENABLE_HUGEPAGE_HINT") {
        mnemosyne_core::options::ENABLE_HUGEPAGE_HINT
            .store(parsed, core::sync::atomic::Ordering::Release);
    }

    if let Some(parsed) = parse_env_usize("MNEMOSYNE_PURGE_CADENCE_MS") {
        mnemosyne_core::options::PURGE_CADENCE_MS
            .store(parsed, core::sync::atomic::Ordering::Release);
        if parsed > 0 {
            mnemosyne_decay::init_decay_engine();
        }
    }

    if let Some(parsed) = parse_env_bool("MNEMOSYNE_PROF") {
        if parsed {
            let interval = parse_env_usize("MNEMOSYNE_PROF_SAMPLE_INTERVAL").unwrap_or(512 * 1024);
            mnemosyne_prof::enable_profiling(interval);
        }
    }

    if let Some(parsed) = parse_env_bool("MNEMOSYNE_LEAK_DETECTOR") {
        if parsed {
            mnemosyne_prof::enable_leak_detector();
        }
    }
}

/// Reset options state and atomic option values to their defaults. Intended for testing.
#[doc(hidden)]
pub fn reset_options_for_testing() {
    OPTIONS_INIT.store(false, core::sync::atomic::Ordering::Release);
    mnemosyne_core::options::MAX_RETAINED_SEGMENTS
        .store(1024, core::sync::atomic::Ordering::Release);
    mnemosyne_core::options::ENABLE_HUGEPAGE_HINT
        .store(true, core::sync::atomic::Ordering::Release);
    mnemosyne_core::options::PURGE_CADENCE_MS.store(0, core::sync::atomic::Ordering::Release);
    mnemosyne_prof::reset_profiler_for_testing();
}

/// Marks options as initialized, preventing subsequent environment parsing from overwriting them.
#[doc(hidden)]
pub fn mark_options_initialized() {
    OPTIONS_INIT.store(true, core::sync::atomic::Ordering::Release);
}

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
    exit_armed: core::cell::Cell<bool>,
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
    fn cache_ptr(&self) -> *mut ThreadAllocator<B> {
        self.allocator.get()
    }
}

#[doc(hidden)]
pub mod internal {
    pub use crate::ensure_options_initialized;
    pub use crate::ThreadAllocator;
    pub use crate::{
        do_local_free_internal, initialize_allocated_bytes, poison_freed_bytes,
        small_realloc_fits_existing_class,
    };
    pub use core::alloc::Layout;
    pub use core::ptr::NonNull;
    pub use mnemosyne_arena::HasSegmentPool;
    pub use mnemosyne_arena::{allocate_large_or_huge, deallocate_large_or_huge};
    pub use mnemosyne_core::constants::{
        MAX_SMALL_ALLOC_SIZE, MIN_BLOCK_SIZE, PAGES_PER_SEGMENT, PAGE_SHIFT, SEGMENT_SIZE,
    };
    pub use mnemosyne_core::size_class::size_to_class_nonzero;
    pub use mnemosyne_core::types::{Block, Page, Segment};
    pub use mnemosyne_core::validation::is_valid_layout_alloc_request;
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
pub unsafe fn initialize_allocated_bytes<P: AllocPolicy>(ptr: *mut u8, size: usize) {
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
pub unsafe fn poison_freed_bytes<P: AllocPolicy>(ptr: *mut u8, size: usize) {
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

    /// Returns the raw pointer to the thread-local allocator cache without triggering lazy initialization.
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void;
}

/// Helper macro to generate zero-cost backend-specific thread-local cache pools.
#[macro_export]
macro_rules! impl_local_allocator_selector {
    ($backend:ty) => {
        const _: () = {
            // Under nightly_tls, we declare ALLOCATOR_SLOT with #[thread_local]
            #[cfg(feature = "nightly_tls")]
            #[thread_local]
            static ALLOCATOR_SLOT: $crate::LocalAllocatorSlot<$backend> =
                $crate::LocalAllocatorSlot::new();

            // Under standard (not nightly_tls), we declare ALLOCATOR_SLOT via std::thread_local!
            #[cfg(not(feature = "nightly_tls"))]
            std::thread_local! {
                static ALLOCATOR_SLOT: $crate::LocalAllocatorSlot<$backend> = const {
                    $crate::LocalAllocatorSlot::new()
                };
            }

            // Expose the slot access cells/guards needed by our TLS strategies.
            std::thread_local! {
                static CACHED_SLOT_PTR: core::cell::Cell<*mut core::ffi::c_void> = const {
                    core::cell::Cell::new(core::ptr::null_mut())
                };

                #[cfg(feature = "nightly_tls")]
                static ALLOCATOR_EXIT_GUARD: $crate::ThreadExitReclaim<$backend> = const {
                    $crate::ThreadExitReclaim::new()
                };
            }

            #[cfg(feature = "nightly_tls")]
            #[thread_local]
            static mut QUICK_ALLOCATOR_PTR: *mut core::ffi::c_void = core::ptr::null_mut();

            static OS_TLS_KEY: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(u32::MAX);

            struct SlotAccess;
            impl $crate::tls::TlsSlotAccess<$backend> for SlotAccess {
                #[inline(always)]
                fn get_slot_standard<R>(f: impl FnOnce(&$crate::LocalAllocatorSlot<$backend>) -> R) -> R {
                    #[cfg(feature = "nightly_tls")]
                    {
                        // In nightly_tls, get_slot_standard falls back to the static reference
                        f(&ALLOCATOR_SLOT)
                    }
                    #[cfg(not(feature = "nightly_tls"))]
                    {
                        ALLOCATOR_SLOT.with(f)
                    }
                }

                #[inline(always)]
                fn get_cached_cell<R>(f: impl FnOnce(&core::cell::Cell<*mut core::ffi::c_void>) -> R) -> R {
                    CACHED_SLOT_PTR.with(f)
                }

                #[inline(always)]
                fn arm_thread_exit(slot: &$crate::LocalAllocatorSlot<$backend>) {
                    #[cfg(feature = "nightly_tls")]
                    {
                        $crate::arm_thread_exit(slot, &ALLOCATOR_EXIT_GUARD);
                    }
                    #[cfg(not(feature = "nightly_tls"))]
                    {
                        // No-op for stable path: LocalAllocatorSlot is registered automatically by standard thread_local!.
                        let _ = slot;
                    }
                }

                #[inline(always)]
                fn get_os_tls_key() -> &'static core::sync::atomic::AtomicU32 {
                    &OS_TLS_KEY
                }

                #[cfg(feature = "nightly_tls")]
                #[inline(always)]
                fn get_slot_nightly<R>(f: impl FnOnce(&$crate::LocalAllocatorSlot<$backend>) -> R) -> R {
                    f(&ALLOCATOR_SLOT)
                }

                #[cfg(feature = "nightly_tls")]
                #[inline(always)]
                fn get_quick_allocator_ptr() -> *mut core::ffi::c_void {
                    unsafe { QUICK_ALLOCATOR_PTR }
                }

                #[cfg(feature = "nightly_tls")]
                #[inline(always)]
                fn set_quick_allocator_ptr(ptr: *mut core::ffi::c_void) {
                    unsafe { QUICK_ALLOCATOR_PTR = ptr; }
                }
            }

            // Statically select the best TLS provider based on compile target and features.
            #[cfg(feature = "nightly_tls")]
            type SelectedTls = $crate::tls::NightlyTls<$backend, SlotAccess>;

            #[cfg(all(not(feature = "nightly_tls"), all(windows, target_arch = "x86_64")))]
            type SelectedTls = $crate::tls::AsmTls<$backend, SlotAccess>;

            #[cfg(all(not(feature = "nightly_tls"), not(all(windows, target_arch = "x86_64"))))]
            type SelectedTls = $crate::tls::NativeOsTls<$backend, SlotAccess>;

            impl $crate::LocalAllocatorSelector<$backend> for $backend {
                #[inline(always)]
                fn with_allocator<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator(f)
                }

                #[inline(always)]
                fn with_allocator_guard<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator_guard(f)
                }

                #[inline(always)]
                unsafe fn with_allocator_unguarded<R>(
                    f: impl FnOnce(&mut $crate::ThreadAllocator<$backend>) -> R,
                ) -> Option<R> {
                    unsafe { <SelectedTls as $crate::tls::TlsProvider<$backend>>::with_allocator_unguarded(f) }
                }

                #[inline(always)]
                fn get_allocator_ptr() -> *mut core::ffi::c_void {
                    $crate::ensure_options_initialized();
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr()
                }

                #[inline(always)]
                fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
                    <SelectedTls as $crate::tls::TlsProvider<$backend>>::get_allocator_ptr_raw()
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
#[inline(always)]
pub unsafe fn usable_size(ptr: *mut u8) -> usize {
    if ptr.is_null() {
        return 0;
    }

    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

    // Safety: for small allocations, page_index is in [1, PAGES_PER_SEGMENT)
    // and the target page records the size-class block size. If page_index is
    // 0 (segment-aligned huge allocation) or the page's block_size is 0
    // (non-segment-aligned huge allocation), we route to the metadata-slot fallback.
    let page = unsafe { (*segment).pages.get_unchecked(page_index) };
    let size = page.block_size;
    if size > 0 {
        return size;
    }

    // Safety: large/huge allocations store the segment pointer in the metadata
    // slot immediately preceding the user pointer.
    let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
    let size = unsafe { (*segment).pages[0].alloc_count };
    if size > 0 {
        size
    } else {
        unsafe { (*segment).huge_mapping_suffix_from(ptr) }
    }
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
#[inline(always)]
pub unsafe fn thread_alloc<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    size: usize,
    align: usize,
) -> *mut u8 {
    if !is_valid_alloc_request(size, align) {
        return core::ptr::null_mut();
    }

    let ptr = unsafe { thread_alloc_checked::<P, B>(size, align) };
    if mnemosyne_prof::is_active() && !ptr.is_null() {
        mnemosyne_prof::on_alloc(ptr, size);
    }
    ptr
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
    let ptr = unsafe { thread_alloc_checked::<P, B>(size, align) };
    if mnemosyne_prof::is_active() && !ptr.is_null() {
        mnemosyne_prof::on_alloc(ptr, size);
    }
    ptr
}

#[inline(always)]
unsafe fn thread_alloc_checked<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    size: usize,
    align: usize,
) -> *mut u8 {
    if align > MIN_BLOCK_SIZE {
        let ptr = unsafe { allocate_large_or_huge::<B>(size, align, P::ENABLE_POISONING) };
        if !ptr.is_null() {
            unsafe { initialize_allocated_bytes::<P>(ptr, size) };
        }
        return ptr;
    }

    let adjusted_size = core::cmp::max(size, align);

    let class = match size_to_class_nonzero(adjusted_size) {
        Some(c) => c,
        None => {
            let ptr =
                unsafe { allocate_large_or_huge::<B>(adjusted_size, align, P::ENABLE_POISONING) };
            if !ptr.is_null() {
                unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
            }
            return ptr;
        }
    };

    let slot_ptr = B::get_allocator_ptr_raw();
    if !slot_ptr.is_null() {
        let alloc = unsafe { &mut *(slot_ptr as *mut ThreadAllocator<B>) };
        if !alloc.is_allocating {
            if let Some(mut page_ptr) = unsafe { *alloc.active_pages.get_unchecked(class) } {
                let page = unsafe { page_ptr.as_mut() };
                if let Some(block) = page.free {
                    let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
                        let self_addr = page as *const Page as usize;
                        let segment_addr = self_addr & !(SEGMENT_SIZE - 1);
                        let segment = segment_addr as *mut Segment;
                        let page_index = page.index_in_segment();
                        unsafe { (*segment).keys[page_index] }
                    } else {
                        0
                    };
                    unsafe {
                        page.free = (*block.as_ptr()).get_next::<P>(cookie);
                    }
                    page.alloc_count += 1;
                    let ptr = block.as_ptr() as *mut u8;
                    unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                    return ptr;
                } else if page.initialized_blocks < page.max_blocks() {
                    let idx = page.initialized_blocks;
                    page.initialized_blocks += 1;
                    page.alloc_count += 1;
                    let page_start = page.page_start();
                    let ptr = unsafe { page_start.add(idx * page.block_size) };
                    unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
                    return ptr;
                }
            }
        }
    }

    unsafe { thread_alloc_cold::<P, B>(class, adjusted_size, align) }
}

#[cold]
#[inline(never)]
unsafe fn thread_alloc_cold<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    class: usize,
    adjusted_size: usize,
    align: usize,
) -> *mut u8 {
    if B::ENABLE_CPU_CACHE {
        let cpu_ptr = per_cpu::try_alloc_cpu::<P>(class);
        if !cpu_ptr.is_null() {
            unsafe { initialize_allocated_bytes::<P>(cpu_ptr, adjusted_size) };
            return cpu_ptr;
        }
    }

    let slot_ptr = B::get_allocator_ptr();
    if slot_ptr.is_null() {
        let ptr = unsafe { allocate_large_or_huge::<B>(adjusted_size, align, P::ENABLE_POISONING) };
        if !ptr.is_null() {
            unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
        }
        return ptr;
    }

    let alloc = unsafe { &mut *(slot_ptr as *mut ThreadAllocator<B>) };
    if alloc.is_allocating {
        let ptr = unsafe { allocate_large_or_huge::<B>(adjusted_size, align, P::ENABLE_POISONING) };
        if !ptr.is_null() {
            unsafe { initialize_allocated_bytes::<P>(ptr, adjusted_size) };
        }
        return ptr;
    }

    alloc.is_allocating = true;
    let ptr = unsafe { alloc.alloc_cold::<P>(class) };
    alloc.is_allocating = false;

    let final_ptr = if ptr.is_null() {
        unsafe { allocate_large_or_huge::<B>(adjusted_size, align, P::ENABLE_POISONING) }
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

    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;

    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

    let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };
    if mnemosyne_prof::is_active() {
        let size = if page_index == 0 || page.block_size == 0 {
            let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            let size = unsafe { (*segment).pages[0].alloc_count };
            if size > 0 {
                size
            } else {
                unsafe { (*segment).huge_mapping_suffix_from(ptr) }
            }
        } else {
            page.block_size
        };
        mnemosyne_prof::on_free(ptr, size);
    }

    if page.block_size == 0 {
        if P::ENABLE_POISONING {
            let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
            let size = unsafe { (*segment).pages[0].alloc_count };
            let size = if size > 0 {
                size
            } else {
                unsafe { (*segment).huge_mapping_suffix_from(ptr) }
            };
            unsafe { poison_freed_bytes::<P>(ptr, size) };
        }
        let segment = unsafe { *((ptr as *mut *mut Segment).sub(1)) };
        let _released = unsafe { deallocate_large_or_huge::<B>(ptr, segment) };
        return;
    }

    debug_assert_eq!(
        (ptr_val & (PAGE_SIZE - 1)) % page.block_size,
        0,
        "small free ptr must be aligned to the page's block stride"
    );

    if P::ENABLE_POISONING {
        unsafe { poison_freed_bytes::<P>(ptr, page.block_size) };
    }

    let block = ptr as *mut Block;
    let owner = unsafe { (*segment).owner };

    #[cfg(all(windows, target_arch = "x86_64"))]
    let is_owner = {
        let tid = unsafe {
            let val: u32;
            core::arch::asm!(
                "mov {0:e}, gs:[0x48]",
                out(reg) val,
                options(nostack, preserves_flags, readonly)
            );
            val
        };
        owner.matches_thread_id(tid)
    };
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    let is_owner = {
        let current_allocator = B::get_allocator_ptr_raw();
        owner.matches(current_allocator)
    };

    if is_owner {
        debug_assert!(page.alloc_count > 0, "local free observed zero alloc_count");
        let page_free = page.free;
        let page_alloc_count = page.alloc_count;
        let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
            unsafe { (*segment).keys[page_index] }
        } else {
            0
        };
        let is_not_full = page.list_state != 2;
        if is_not_full && (page_alloc_count != 1 || unsafe { (*segment).is_current }) {
            unsafe {
                (*block).set_next::<P>(page_free, cookie);
                page.free = Some(NonNull::new_unchecked(block));
                page.alloc_count = page_alloc_count - 1;
            }
            return;
        }

        let current_allocator = B::get_allocator_ptr_raw();
        if !current_allocator.is_null() {
            let alloc = unsafe { &mut *(current_allocator as *mut ThreadAllocator<B>) };
            if !alloc.is_allocating {
                alloc.is_allocating = true;
                do_local_free_internal::<P, B>(alloc, block, page, segment);
                alloc.is_allocating = false;
                return;
            }
        }
    }

    unsafe { thread_free_cold::<P, B>(ptr, page, block) };
}

#[cold]
#[inline(never)]
unsafe fn thread_free_cold<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
    page: &mut Page,
    block: *mut Block,
) {
    if B::ENABLE_CPU_CACHE && per_cpu::try_free_cpu::<P>(ptr, page.size_class as usize) {
        return;
    }

    unsafe {
        page.thread_free.push::<P>(NonNull::new_unchecked(block));
    }
}

/// Internal implementation of local deallocation.
///
/// # Safety
///
/// The block pointer must point to a valid block allocated in the target page and segment.
#[inline(always)]
pub unsafe fn do_local_free_internal<P: AllocPolicy, B: HasSegmentPool>(
    alloc: &mut ThreadAllocator<B>,
    block: *mut Block,
    page: &mut Page,
    segment: *mut Segment,
) {
    let was_full = page.list_state == 2;
    let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
        let page_index = page.index_in_segment();
        unsafe { (*segment).keys[page_index] }
    } else {
        0
    };
    unsafe {
        (*block).set_next::<P>(page.free, cookie);
    }
    page.free = Some(NonNull::new_unchecked(block));

    page.alloc_count -= 1;
    let becomes_empty = page.alloc_count == 0;

    if was_full {
        let class = page.size_class as usize;
        if alloc.unlink_full_page(page as *mut Page, class) {
            unsafe {
                alloc.push_active_page(NonNull::new_unchecked(page as *mut Page), class);
            }
        }
    }
    if becomes_empty && !alloc.is_current_segment(segment) {
        let class = page.size_class as usize;
        let is_only_active = unsafe {
            alloc.active_pages.get_unchecked(class).is_some_and(|head| {
                core::ptr::eq(head.as_ptr(), page as *const Page) && page.next_page.is_none()
            })
        };
        if !is_only_active && !alloc.try_reclaim_segment(segment) {
            alloc.unlink_page(page as *mut Page, class);
            unsafe {
                alloc.push_empty_page(NonNull::new_unchecked(page as *mut Page));
            }
        }
    }
}

#[inline(always)]
pub fn small_realloc_fits_existing_class(layout: Layout, new_size: usize) -> bool {
    if layout.align() > MIN_BLOCK_SIZE {
        return false;
    }

    let old_adjusted_size = core::cmp::max(layout.size(), layout.align());
    if old_adjusted_size <= 128 {
        new_size <= (old_adjusted_size + 15) & !15
    } else if old_adjusted_size <= 512 {
        new_size <= (old_adjusted_size + 31) & !31
    } else if old_adjusted_size <= 2048 {
        new_size <= (old_adjusted_size + 127) & !127
    } else if old_adjusted_size <= MAX_SMALL_ALLOC_SIZE {
        new_size <= (old_adjusted_size + 511) & !511
    } else {
        false
    }
}

/// Reallocates a memory block, optimizing performance and memory footprint by avoiding redundant
/// allocation-deallocation cycles, reusing existing size-class blocks in place, and reducing TLS
/// lookup overhead.
///
/// # Safety
///
/// Same contract as `GlobalAlloc::realloc`.
#[inline]
pub unsafe fn thread_realloc<P: AllocPolicy, B: HasSegmentPool + LocalAllocatorSelector<B>>(
    ptr: *mut u8,
    layout: Layout,
    new_size: usize,
) -> *mut u8 {
    if !ptr.is_null() && new_size != 0 {
        if !P::ZERO_INITIALIZE && !P::ENABLE_POISONING {
            if new_size <= layout.size() {
                if layout.size() <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                    if new_size >= layout.size() / 2 {
                        return ptr;
                    }
                } else {
                    let current_usable = unsafe { usable_size(ptr) };
                    let new_adjusted = core::cmp::max(new_size, layout.align());
                    if new_adjusted <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                        if new_size >= layout.size() / 2 {
                            return ptr;
                        }
                    } else if new_size >= layout.size() / 2 {
                        return ptr;
                    } else {
                        let page_size = mnemosyne_core::constants::PAGE_SIZE;
                        let new_page_rounded = (new_adjusted + page_size - 1) & !(page_size - 1);
                        if new_page_rounded >= current_usable {
                            return ptr;
                        }
                    }
                }
            } else {
                if layout.size() <= MAX_SMALL_ALLOC_SIZE && layout.align() <= MIN_BLOCK_SIZE {
                    if small_realloc_fits_existing_class(layout, new_size) {
                        return ptr;
                    }
                } else {
                    let current_usable = unsafe { usable_size(ptr) };
                    if new_size <= current_usable {
                        return ptr;
                    }
                }
            }
        }
    } else {
        if ptr.is_null() {
            if new_size == 0 {
                return core::ptr::null_mut();
            }
            return unsafe { thread_alloc_layout::<P, B>(new_size, layout.align()) };
        }
        // new_size == 0 && !ptr.is_null()
        unsafe { thread_free::<P, B>(ptr) };
        return core::ptr::null_mut();
    }

    let new_adjusted = core::cmp::max(new_size, layout.align());
    let new_class = size_to_class_nonzero(new_adjusted);

    let ptr_val = ptr as usize;
    let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
    let segment = segment_addr as *mut Segment;
    let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

    let page = unsafe { (*segment).pages.get_unchecked_mut(page_index) };
    let is_old_small = page_index > 0 && page.block_size > 0;

    let mut new_ptr = core::ptr::null_mut();
    let mut local_free_done = false;

    if is_old_small {
        if let Some(class) = new_class {
            let _ = B::with_allocator_guard(|alloc| {
                #[cfg(all(windows, target_arch = "x86_64"))]
                let is_owner = {
                    let tid = unsafe {
                        let val: u32;
                        core::arch::asm!(
                            "mov {0:e}, gs:[0x48]",
                            out(reg) val,
                            options(nostack, preserves_flags, readonly)
                        );
                        val
                    };
                    unsafe { (*segment).owner.matches_thread_id(tid) }
                };
                #[cfg(not(all(windows, target_arch = "x86_64")))]
                let is_owner = {
                    let current_ptr = alloc as *mut ThreadAllocator<B> as *mut core::ffi::c_void;
                    unsafe { (*segment).owner.matches(current_ptr) }
                };

                if is_owner {
                    let allocated = unsafe { alloc.alloc_class::<P>(class) };
                    new_ptr = allocated;
                    if !new_ptr.is_null() {
                        unsafe {
                            initialize_allocated_bytes::<P>(new_ptr, new_adjusted);
                            core::ptr::copy_nonoverlapping(
                                ptr,
                                new_ptr,
                                core::cmp::min(layout.size(), new_size),
                            );
                            let page_ref = &mut *page;
                            if P::ENABLE_POISONING {
                                poison_freed_bytes::<P>(ptr, page_ref.block_size);
                            }
                            let block = ptr as *mut Block;
                            let page_free = page_ref.free;
                            let page_alloc_count = page_ref.alloc_count;
                            let cookie = if P::ENABLE_FREE_LIST_ENCRYPTION {
                                (*segment).keys[page_index]
                            } else {
                                0
                            };
                            if page_free.is_some()
                                && (page_alloc_count != 1 || (*segment).is_current)
                            {
                                (*block).set_next::<P>(page_free, cookie);
                                page_ref.free = Some(NonNull::new_unchecked(block));
                                page_ref.alloc_count = page_alloc_count - 1;
                            } else {
                                do_local_free_internal::<P, B>(alloc, block, page_ref, segment);
                            }
                        }
                        local_free_done = true;
                    }
                }
            });
        }
    }

    if new_ptr.is_null() {
        new_ptr = unsafe { thread_alloc_layout::<P, B>(new_size, layout.align()) };
        if new_ptr.is_null() {
            return core::ptr::null_mut();
        }
    }

    if !local_free_done {
        unsafe {
            core::ptr::copy_nonoverlapping(ptr, new_ptr, core::cmp::min(layout.size(), new_size));
            thread_free::<P, B>(ptr);
        }
    }

    new_ptr
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
            let ptr_val = ptr as usize;
            let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;
            let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
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
                mnemosyne_arena::allocate_large_or_huge::<MemoryBackendWrapper>(
                    request, align, true,
                )
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
                mnemosyne_arena::allocate_large_or_huge::<MemoryBackendWrapper>(
                    request, align, true,
                )
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

            let ptr_val = ptr as usize;
            let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
            let segment = segment_addr as *mut Segment;
            let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);

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
            let offset = ptr_val & (PAGE_SIZE - 1);
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

        let ptr_val = ptr as usize;
        let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
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

    #[test]
    fn hardened_policy_round_trip_alloc_free() {
        use mnemosyne_hardened::HardenedPolicy;

        let _guard = crate::local_alloc::TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");

        let ptr = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(32, 8) };
        assert!(!ptr.is_null(), "HardenedPolicy small allocation failed");

        // Verify that the memory is zero-initialized (since HardenedPolicy inherits from SecurePolicy, which zero-initializes)
        let slice = unsafe { core::slice::from_raw_parts(ptr, 32) };
        for &byte in slice {
            assert_eq!(
                byte, 0,
                "HardenedPolicy allocation was not zero-initialized"
            );
        }

        // Verify that we can write to it
        unsafe {
            core::ptr::write_bytes(ptr, 0x42, 32);
        }

        // Free the pointer
        unsafe {
            thread_free::<HardenedPolicy, MemoryBackendWrapper>(ptr);
        }
    }

    #[test]
    fn hardened_policy_detects_freelist_tamper() {
        use mnemosyne_hardened::HardenedPolicy;

        let _guard = crate::local_alloc::TEST_LOCK
            .lock()
            .expect("local allocator test lock was poisoned");

        // We want to verify that tamper detection works under HardenedPolicy.
        // Let's allocate two blocks on a fresh page of class 0 (16 bytes).
        // Since we want them on the same page, we can allocate them in sequence.
        let ptr1 = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(16, 8) };
        let ptr2 = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(16, 8) };
        assert!(!ptr1.is_null());
        assert!(!ptr2.is_null());

        // Free them in sequence so they end up in the thread-local free list
        unsafe {
            thread_free::<HardenedPolicy, MemoryBackendWrapper>(ptr1);
            thread_free::<HardenedPolicy, MemoryBackendWrapper>(ptr2);
        }

        // Now, `page.free` points to `ptr2`, and `ptr2` contains the encrypted pointer to `ptr1`.
        // Let's tamper with the encrypted next pointer in `ptr2`.
        // The block metadata stores the encrypted pointer in the first `Option<NonNull<Block>>` slot of the block.
        let val2 = ptr2 as *mut usize;
        unsafe {
            let original_val = *val2;
            // Corrupt the pointer (e.g. flip a bit in the address portion)
            *val2 = original_val ^ 0x08;
        }

        // Now, try to allocate. The first allocation gets `ptr2` (which is successful).
        let ptr3 = unsafe { thread_alloc::<HardenedPolicy, MemoryBackendWrapper>(16, 8) };
        assert_eq!(ptr3, ptr2);

        // The second allocation would follow the tampered pointer to `ptr1`.
        // Since we flipped a bit, the decrypted address is incorrect and fails to match `ptr1`.
        // In particular, the page's free pointer now contains garbage.
        let ptr_val = ptr3 as usize;
        let segment_addr = ptr_val & !(SEGMENT_SIZE - 1);
        let segment = segment_addr as *mut Segment;
        let page_index = (ptr_val >> PAGE_SHIFT) & (PAGES_PER_SEGMENT - 1);
        let page = unsafe { (*segment).pages.get_unchecked(page_index) };

        let free_head = page.free.map(|p| p.as_ptr() as usize);
        assert_ne!(
            free_head,
            Some(ptr1 as usize),
            "HardenedPolicy failed to obscure/randomize the tampered pointer"
        );
    }

    #[test]
    fn test_dealloc_path() {
        let ptr = unsafe { thread_alloc::<StandardPolicy, MemoryBackendWrapper>(1024, 8) };
        assert!(!ptr.is_null());
        unsafe { thread_free::<StandardPolicy, MemoryBackendWrapper>(ptr) };
    }
}
