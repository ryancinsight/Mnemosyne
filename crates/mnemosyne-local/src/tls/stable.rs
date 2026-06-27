//! Stable-channel TLS providers using `thread_local!` macros.
//!
//! `StandardTls` accesses the slot through a direct `thread_local!` lookup.
//! `CachedCellTls` caches the raw allocator pointer in a `Cell<*mut c_void>` to
//! bypass lazy-initialization overhead on hot paths.

use super::traits::{TlsProvider, TlsSlotAccess};
use crate::ThreadAllocator;
use mnemosyne_arena::HasSegmentPool;

/// Portable TLS provider using direct standard `std::thread_local!` lookups.
pub struct StandardTls<B, S>(core::marker::PhantomData<(B, S)>);

impl<B: HasSegmentPool, S: TlsSlotAccess<B>> TlsProvider<B> for StandardTls<B, S> {
    const IDENTIFIER: &'static str = "StandardTls";

    #[inline(always)]
    fn with_allocator<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        S::get_slot_standard(|slot| {
            S::arm_thread_exit(slot);
            slot.with_allocator(f)
        })
    }

    #[inline(always)]
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        S::get_slot_standard(|slot| {
            S::arm_thread_exit(slot);
            slot.with_allocator(f)
        })
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        S::get_slot_standard(|slot| unsafe { slot.with_allocator_unguarded(f) })
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
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            alloc.is_allocating = true;
            let result = f(alloc);
            alloc.is_allocating = false;
            Some(result)
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                S::get_cached_cell(|cell| cell.set(alloc_ptr));
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            })
        }
    }

    #[inline(always)]
    fn with_allocator_guard<R>(f: impl FnOnce(&mut ThreadAllocator<B>) -> R) -> Option<R> {
        let ptr = S::get_cached_cell(|cell| cell.get());
        if !ptr.is_null() {
            // SAFETY: `ptr` is this thread's own allocator pointer cached in its
            // thread-local cell (written in the init branch below); no other
            // thread aliases it. `is_allocating` gates same-thread re-entry, so
            // no second live `&mut` to the cache can be created.
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            alloc.is_allocating = true;
            let result = f(alloc);
            alloc.is_allocating = false;
            Some(result)
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                S::get_cached_cell(|cell| cell.set(alloc_ptr));
                S::arm_thread_exit(slot);
                slot.with_allocator(f)
            })
        }
    }

    #[inline(always)]
    unsafe fn with_allocator_unguarded<R>(
        f: impl FnOnce(&mut ThreadAllocator<B>) -> R,
    ) -> Option<R> {
        let ptr = S::get_cached_cell(|cell| cell.get());
        if !ptr.is_null() {
            // SAFETY: `ptr` is this thread's own allocator pointer cached in its
            // thread-local cell; the pointee is exclusive to the current thread.
            // `is_allocating` gates same-thread re-entry, and the caller of this
            // `unsafe fn` upholds the no-re-entry contract of
            // `with_allocator_unguarded`, so no aliasing `&mut` can be formed.
            let alloc = unsafe { &mut *(ptr as *mut ThreadAllocator<B>) };
            if alloc.is_allocating {
                return None;
            }
            Some(f(alloc))
        } else {
            S::get_slot_standard(|slot| {
                let alloc_ptr = slot.allocator_ptr();
                S::get_cached_cell(|cell| cell.set(alloc_ptr));
                S::arm_thread_exit(slot);
                unsafe { slot.with_allocator_unguarded(f) }
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
