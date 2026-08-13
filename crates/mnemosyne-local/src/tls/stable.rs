//! Stable-channel TLS providers using `thread_local!` macros.
//!
//! `StandardTls` accesses the slot through a direct `thread_local!` lookup.
//! `CachedCellTls` caches the raw allocator pointer in a `Cell<*mut c_void>` to
//! bypass lazy-initialization overhead on hot paths.

use super::traits::{TlsProvider, TlsSlotAccess};
use crate::ThreadAllocator;
use crate::tls_slot::LocalAllocatorSlot;
use mnemosyne_arena::HasSegmentPool;

/// Portable TLS provider using direct standard `std::thread_local!` lookups.
pub struct StandardTls<B, S>(core::marker::PhantomData<(B, S)>);

impl<B: HasSegmentPool, S: TlsSlotAccess<B>> TlsProvider<B> for StandardTls<B, S> {
    const IDENTIFIER: &'static str = "StandardTls";

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        S::get_slot_standard(|slot| {
            S::arm_thread_exit(slot);
            // SAFETY: `allocator_ptr` returns this slot's own live address.
            unsafe { LocalAllocatorSlot::<B>::with_allocator(slot.allocator_ptr(), f) }
        })
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        S::get_slot_standard(|slot| {
            // SAFETY: `allocator_ptr` returns this slot's own live address, and
            // the caller's no-re-entry contract is forwarded unchanged.
            unsafe { LocalAllocatorSlot::<B>::with_allocator_unguarded(slot.allocator_ptr(), f) }
        })
    }

    #[inline(always)]
    fn get_allocator_ptr() -> *mut core::ffi::c_void {
        S::get_slot_standard(|slot| slot.allocator_ptr())
    }

    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        S::get_slot_standard(|slot| slot.allocator_ptr())
    }
}

/// Portable TLS provider that caches the raw slot pointer in a standard `thread_local!` `Cell`.
///
/// Bypasses lazy-initialization overhead of the full allocator slot on subsequent accesses.
pub struct CachedCellTls<B, S>(core::marker::PhantomData<(B, S)>);

impl<B: HasSegmentPool, S: TlsSlotAccess<B>> TlsProvider<B> for CachedCellTls<B, S> {
    const IDENTIFIER: &'static str = "CachedCellTls";

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        let ptr = S::get_cached_cell(|cell| cell.get());
        if !ptr.is_null() {
            // SAFETY: a non-null `ptr` in this thread's cache cell was written by
            // this thread's own `slot.allocator_ptr()` in the init branch below.
            // The cell is a thread-local `Cell`, so the pointee is exclusive to
            // the current thread (no cross-thread aliasing); `is_allocating`
            // rejects nested same-thread access before a second `&mut` exists.
            // The cached value is the slot address, which by the slot's
            // offset-0 invariant is the same value as the allocator address the
            // owner token uses — so this reinterpretation changes no token.
            // SAFETY: `ptr` is this thread's own slot address cached below.
            unsafe { LocalAllocatorSlot::<B>::with_allocator(ptr, f) }
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                S::get_cached_cell(|cell| cell.set(alloc_ptr));
                S::arm_thread_exit(slot);
                // SAFETY: `allocator_ptr` returns this slot's own live address.
                unsafe { LocalAllocatorSlot::<B>::with_allocator(slot.allocator_ptr(), f) }
            })
        }
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        let ptr = S::get_cached_cell(|cell| cell.get());
        if !ptr.is_null() {
            // The re-entry flag is read through a raw projection, before any
            // reference exists — forming `&mut` first and then testing the flag
            // commits the exact aliasing the flag exists to reject.
            //
            // This ordering is necessary but NOT sufficient, and the code is
            // still unsound on the re-entrant path: a strongly-protected `&mut`
            // excludes *all* access through other tags, so even this read
            // conflicts with a live outer borrow. Miri reports it from
            // `unguarded_fast_path_rejects_reentrant_borrow`. The flag cannot
            // fix this while it lives inside the object it guards; relocating it
            // out of `ThreadAllocator` is tracked as MN-440.
            //
            // SAFETY: `ptr` is this thread's own allocator pointer cached in its
            // thread-local cell; the pointee is exclusive to the current thread,
            // so projecting to one field and reading it is valid.
            // SAFETY: `ptr` is this thread's own slot address cached below; the
            // caller upholds `with_allocator_unguarded`'s no-re-entry contract.
            unsafe { LocalAllocatorSlot::<B>::with_allocator_unguarded(ptr, f) }
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                S::get_cached_cell(|cell| cell.set(alloc_ptr));
                S::arm_thread_exit(slot);
                unsafe { LocalAllocatorSlot::<B>::with_allocator_unguarded(ptr, f) }
            })
        }
    }

    #[inline(always)]
    fn get_allocator_ptr() -> *mut core::ffi::c_void {
        let ptr = S::get_cached_cell(|cell| cell.get());
        if !ptr.is_null() {
            ptr
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                S::get_cached_cell(|cell| cell.set(alloc_ptr));
                alloc_ptr
            })
        }
    }

    #[inline(always)]
    fn get_allocator_ptr_raw() -> *mut core::ffi::c_void {
        S::get_cached_cell(|cell| cell.get())
    }
}
