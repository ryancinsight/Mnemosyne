//! Page-level OS allocation and release for the `MemoryBackendWrapper`.
//!
//! Owns the [`MemoryBackendWrapper`] struct, the capability consts that
//! forward the platform-conditional [`crate::DefaultBackend`] feature
//! surface, and the single central
//! `impl MemoryBackend for MemoryBackendWrapper` block. Per-method
//! bodies delegate to per-concern helpers:
//!
//! - `do_allocate`/`do_deallocate` (mapping concern) live here.
//! - `crate::guard::do_make_guard` handles `make_guard`.
//! - `crate::reset::do_page_reset` / `crate::reset::do_decommit`
//!   handle `page_reset` / `decommit`.
//!
//! Rust's trait coherence rule keeps the `impl` block in one file; the
//! `#[inline(always)]` glue keeps the per-method delegation statically
//! dispatched with no vtable or heap allocation. Benchmark threshold
//! gates are the empirical evidence for non-regression.

use crate::recorders::{record_map, record_unmap, record_unmap_failure};
use crate::DefaultBackend;
use mnemosyne_core::MemoryBackend;

/// High-level OS page mapping backend helper. Owns the wrapper struct
/// shape and forwards the platform-conditional capability consts from
/// [`crate::DefaultBackend`]; per-method bodies delegate to
/// per-concern helpers in [`crate::guard`] and [`crate::reset`].
pub struct MemoryBackendWrapper;

/// Performs the allocate-side work for [`MemoryBackendWrapper`]:
/// delegate the platform call to `B`, then forward the confirmed
/// mapping to [`crate::recorders::record_map`].
///
/// `#[inline(always)]` keeps this wrapper statically dispatched at
/// the call site.
#[inline(always)]
pub(crate) fn do_allocate<B: MemoryBackend>(size: usize) -> *mut u8 {
    // Safety: forwarded to the platform backend; the size contract
    // (page-aligned, non-zero) is upheld by the trait-level safety
    // expectation on `allocate`.
    let ptr = unsafe { B::allocate(size) };
    if !ptr.is_null() {
        record_map(size);
    }
    ptr
}

/// Performs the deallocate-side work for [`MemoryBackendWrapper`]:
/// delegate the platform call to `B`, then route the outcome to
/// [`crate::recorders::record_unmap`] on a confirmed release or
/// [`crate::recorders::record_unmap_failure`] on a failed release
/// (so `current_mapped_bytes` stays consistent with the live mapping
/// set).
///
/// `#[inline(always)]` keeps this wrapper statically dispatched at
/// the call site.
#[inline(always)]
pub(crate) fn do_deallocate<B: MemoryBackend>(ptr: *mut u8, size: usize) -> bool {
    if ptr.is_null() {
        return false;
    }
    let released = unsafe { B::deallocate(ptr, size) };
    if released {
        record_unmap(size);
    } else {
        record_unmap_failure();
    }
    released
}

impl MemoryBackend for MemoryBackendWrapper {
    const SUPPORTS_PAGE_RESET: bool = DefaultBackend::SUPPORTS_PAGE_RESET;
    const SUPPORTS_MAKE_GUARD: bool = DefaultBackend::SUPPORTS_MAKE_GUARD;
    const SUPPORTS_DECOMMIT: bool = DefaultBackend::SUPPORTS_DECOMMIT;
    const ENABLE_CPU_CACHE: bool = DefaultBackend::ENABLE_CPU_CACHE;

    /// Allocates memory from the OS.
    ///
    /// # Safety
    ///
    /// Size must be greater than zero and page-aligned.
    #[inline(always)]
    unsafe fn allocate(size: usize) -> *mut u8 {
        do_allocate::<DefaultBackend>(size)
    }

    /// Releases memory to the OS.
    ///
    /// # Safety
    ///
    /// The ptr must be valid and size must match the allocated size.
    #[inline(always)]
    unsafe fn deallocate(ptr: *mut u8, size: usize) -> bool {
        do_deallocate::<DefaultBackend>(ptr, size)
    }

    /// Installs a `PROT_NONE` / `PAGE_NOACCESS` guard region.
    ///
    /// Delegates to `crate::guard::do_make_guard` which records the
    /// confirmed install through `crate::recorders::record_guard_install`.
    #[inline(always)]
    unsafe fn make_guard(ptr: *mut u8, size: usize) -> bool {
        crate::guard::do_make_guard::<DefaultBackend>(ptr, size)
    }

    /// Drops the physical backing of an idle page range while keeping
    /// the virtual mapping committed.
    ///
    /// Delegates to `crate::reset::do_page_reset` which records the
    /// confirmed reset through `crate::recorders::record_page_reset`.
    #[inline(always)]
    unsafe fn page_reset(ptr: *mut u8, size: usize) -> bool {
        crate::reset::do_page_reset::<DefaultBackend>(ptr, size)
    }

    /// Releases the commit charge / physical backing of a page-aligned
    /// range while keeping the reservation.
    ///
    /// Delegates to `crate::reset::do_decommit` which records the
    /// confirmed decommit through `crate::recorders::record_decommit`.
    #[inline(always)]
    unsafe fn decommit(ptr: *mut u8, size: usize) -> bool {
        crate::reset::do_decommit::<DefaultBackend>(ptr, size)
    }
}
